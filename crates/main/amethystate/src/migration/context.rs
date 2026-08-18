use crate::MigrationError;
use crate::codec::CodecError;
use crate::migration::fields::AmeStateFields;
use crate::migration::migrate_from::MigrateFrom;
use crate::store::MigrationBackendAdapter;
use crate::store::{CodecFormat, StorageResult};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::hash::Hash;
use std::str::FromStr;

pub struct MigrationContext<'a> {
    prefix: String,
    storage: &'a mut dyn MigrationBackendAdapter,
}

impl<'a> MigrationContext<'a> {
    /// Builds a context over one prefix. The engine does this; a migration
    /// step receives the result.
    pub fn new(prefix: String, storage: &'a mut dyn MigrationBackendAdapter) -> Self {
        Self { prefix, storage }
    }

    /// Migrates a nested struct held at `key`, running its own
    /// [`MigrateFrom`] and returning the new shape.
    ///
    /// For a field that is itself an `#[amethystate]` struct, so its migration
    /// is written once and reused wherever it is nested.
    pub fn nested<TOld, TNew>(&mut self, key: &str, old_data: TOld) -> StorageResult<TNew>
    where
        TOld: AmeStateFields,
        TNew: MigrateFrom<TOld> + AmeStateFields,
    {
        let mut sub_ctx = self.scoped(key);

        let new_data = TNew::migrate(old_data, &mut sub_ctx)?;

        for old_f in TOld::FIELDS {
            let is_renamed = TNew::RENAMES.iter().any(|(ok, _)| *ok == old_f.name);
            let is_kept = TNew::FIELDS.iter().any(|nf| nf.name == old_f.name);

            if is_renamed || !is_kept {
                sub_ctx.delete(old_f.name)?;
            }
        }
        Ok(new_data)
    }

    /// Removes a key that the new schema no longer has.
    ///
    /// Deleting a key that was never there is not an error - a migration has
    /// to survive running against data that skipped a version.
    pub fn delete(&mut self, key: &str) -> StorageResult<()> {
        self.storage.delete(&self.scoped_path(key))
    }

    /// Moves a value to another key, bytes untouched.
    ///
    /// Nothing is decoded, so this works whatever the value's type and cannot
    /// fail on a type it does not know. A `from` that holds nothing is a
    /// no-op rather than an error.
    pub fn rename(&mut self, from: &str, to: &str) -> StorageResult<()> {
        if let Some(bytes) = self.get_raw(from)? {
            self.set_raw(to, &bytes)?;
            self.delete(from)?;
        }
        Ok(())
    }

    /// Reads a value as `TOld`, hands it to `f`, and writes the result back
    /// as `TNew`.
    ///
    /// The one to reach for when a field changes type or representation but
    /// keeps its place.
    pub fn transform<TOld, TNew>(
        &mut self,
        key: &str,
        f: impl FnOnce(TOld) -> StorageResult<TNew>,
    ) -> StorageResult<()>
    where
        TOld: DeserializeOwned,
        TNew: Serialize,
    {
        if let Some(old_val) = self.get::<TOld>(key)? {
            let new_val = f(old_val)?;
            self.set(key, &new_val)?;
        }
        Ok(())
    }

    /// Folds two keys into one: reads both, hands them to `f`, writes the
    /// result at `into` and drops the sources.
    pub fn merge<TOld1, TOld2, TNew>(
        &mut self,
        from: (&str, &str),
        into: &str,
        f: impl FnOnce(TOld1, TOld2) -> StorageResult<TNew>,
    ) -> StorageResult<()>
    where
        TOld1: DeserializeOwned,
        TOld2: DeserializeOwned,
        TNew: Serialize,
    {
        if let (Some(v1), Some(v2)) = (self.get::<TOld1>(from.0)?, self.get::<TOld2>(from.1)?) {
            let new_val = f(v1, v2)?;
            self.set(into, &new_val)?;
            self.delete(from.0)?;
            self.delete(from.1)?;
        }
        Ok(())
    }

    /// The inverse of [`MigrationContext::merge`]: reads one key, hands it to
    /// `f`, and writes the pair it returns to two keys, dropping the source.
    pub fn split<TOld, TNew1, TNew2>(
        &mut self,
        from: &str,
        into: (&str, &str),
        f: impl FnOnce(TOld) -> StorageResult<(TNew1, TNew2)>,
    ) -> StorageResult<()>
    where
        TOld: DeserializeOwned,
        TNew1: Serialize,
        TNew2: Serialize,
    {
        if let Some(old_val) = self.get::<TOld>(from)? {
            let (v1, v2) = f(old_val)?;
            self.set(into.0, &v1)?;
            self.set(into.1, &v2)?;
            self.delete(from)?;
        }
        Ok(())
    }

    /// Reads a value as `T`, relative to this context's prefix.
    ///
    /// The escape hatch for a migration the shaped helpers do not cover.
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> StorageResult<Option<T>> {
        match self.get_raw(key)? {
            Some(bytes) => Ok(Some(decode(self.storage, &bytes)?)),
            None => Ok(None),
        }
    }

    /// Writes a value relative to this context's prefix.
    pub fn set<T: Serialize>(&mut self, key: &str, value: &T) -> StorageResult<()> {
        self.set_raw(key, &encode(self.storage, value)?)
    }

    /// Reads a value by its whole path, ignoring this context's prefix.
    ///
    /// For a step that needs something another part of the store owns. Whether
    /// that value has already been migrated depends on ordering - declare it
    /// with [`PrefixMigrationBuilder::depends_on`](crate::migration::builder::PrefixMigrationBuilder::depends_on)
    /// rather than hoping.
    pub fn global_get<T: DeserializeOwned>(&self, full_key: &str) -> StorageResult<Option<T>> {
        match self.storage.get(full_key)? {
            Some(bytes) => Ok(Some(decode(self.storage, &bytes)?)),
            None => Ok(None),
        }
    }

    /// Writes a value by its whole path, ignoring this context's prefix.
    pub fn global_set<T: Serialize>(&mut self, full_key: &str, value: &T) -> StorageResult<()> {
        let bytes = encode(self.storage, value)?;
        self.storage.set(full_key, &bytes)
    }

    /// The stored bytes at `key`, undecoded.
    ///
    /// For moving a value whose type this step cannot name, or reading one
    /// written in a shape that no longer deserialises.
    pub fn get_raw(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        self.storage.get(&self.scoped_path(key))
    }

    /// Writes bytes at `key` as they are.
    ///
    /// They must be in the backend's own encoding - [`encode`] produces it.
    pub fn set_raw(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
        self.storage.set(&self.scoped_path(key), value)
    }

    /// A context narrowed to a sub-prefix, so a nested part can be migrated
    /// with keys relative to it.
    pub fn scoped(&mut self, sub_prefix: &str) -> MigrationContext<'_> {
        MigrationContext {
            prefix: self.scoped_path(sub_prefix),
            storage: self.storage,
        }
    }

    /// Reads a whole [`ReactiveMap`](crate::ReactiveMap) at `key` as a plain
    /// map, so a step can rewrite its entries.
    pub fn scan_map<K, V>(&self, key: &str) -> StorageResult<HashMap<K, V>>
    where
        K: FromStr + Eq + Hash,
        V: DeserializeOwned,
    {
        let full_prefix = format!("{}.", self.scoped_path(key));
        let raw = self.storage.scan_prefix(&full_prefix)?;
        let mut map = HashMap::new();
        for (path, bytes) in raw {
            if let Some(k_str) = path.strip_prefix(&full_prefix)
                && let Ok(kv) = K::from_str(k_str)
                && let Ok(vv) = decode::<V>(self.storage, &bytes)
            {
                map.insert(kv, vv);
            }
        }
        Ok(map)
    }

    fn scoped_path(&self, key: &str) -> String {
        if self.prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}.{}", self.prefix, key)
        }
    }
}

/// Serialises a value in the backend's own format, for handing to
/// [`MigrationContext::set_raw`].
pub fn encode<T: Serialize>(
    storage: &dyn MigrationBackendAdapter,
    value: &T,
) -> StorageResult<Vec<u8>> {
    match storage.format() {
        #[cfg(feature = "redb")]
        CodecFormat::MessagePack => rmp_serde::to_vec(value)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "json")]
        CodecFormat::Json => serde_json::to_vec(value)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "toml")]
        CodecFormat::Toml => {
            #[derive(serde::Serialize)]
            struct Wrap<'a, T> {
                val: &'a T,
            }
            toml_edit::ser::to_string(&Wrap { val: value })
                .map(|s| s.into_bytes())
                .map_err(|e| CodecError::Toml(e.to_string()))
                .map_err(MigrationError::from)
                .map_err(Into::into)
        }
        #[cfg(feature = "sqlite")]
        CodecFormat::SonicJson => sonic_rs::to_vec(value)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "ron")]
        CodecFormat::Ron => ron::to_string(value)
            .map(|s| s.into_bytes())
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(test)]
        CodecFormat::Default => serde_json::to_vec(value)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),
    }
}

pub fn decode<T: DeserializeOwned>(
    storage: &dyn MigrationBackendAdapter,
    bytes: &[u8],
) -> StorageResult<T> {
    match storage.format() {
        #[cfg(feature = "redb")]
        CodecFormat::MessagePack => rmp_serde::from_slice(bytes)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "json")]
        CodecFormat::Json => serde_json::from_slice(bytes)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "toml")]
        CodecFormat::Toml => {
            #[derive(serde::Deserialize)]
            struct Unwrap<T> {
                val: T,
            }
            toml_edit::de::from_slice::<Unwrap<T>>(bytes)
                .map(|unwrapped| unwrapped.val)
                .map_err(|e| CodecError::Toml(e.to_string()))
                .map_err(MigrationError::from)
                .map_err(Into::into)
        }
        #[cfg(feature = "sqlite")]
        CodecFormat::SonicJson => sonic_rs::from_slice(bytes)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(feature = "ron")]
        CodecFormat::Ron => ron::de::from_bytes(bytes)
            .map_err(|e| CodecError::from(e.code))
            .map_err(MigrationError::from)
            .map_err(Into::into),

        #[cfg(test)]
        CodecFormat::Default => serde_json::from_slice(bytes)
            .map_err(CodecError::from)
            .map_err(MigrationError::from)
            .map_err(Into::into),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migration::AppliedStep;
    use crate::store::meta::{PrefixMeta, SchemaSnapshot};
    use std::collections::HashMap;

    struct MemoryStorage {
        data: HashMap<String, Vec<u8>>,
    }

    impl MigrationBackendAdapter for MemoryStorage {
        fn format(&self) -> CodecFormat {
            CodecFormat::Default
        }

        fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
            Ok(self.data.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
            self.data.insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&mut self, key: &str) -> StorageResult<()> {
            self.data.remove(key);
            Ok(())
        }

        fn scan_prefix(&self, _: &str) -> StorageResult<Vec<(String, Vec<u8>)>> {
            unreachable!()
        }

        fn get_meta(&self, _prefix: &str) -> StorageResult<Option<PrefixMeta>> {
            unreachable!()
        }

        fn set_meta(&mut self, _prefix: &str, _meta: &PrefixMeta) -> StorageResult<()> {
            unreachable!()
        }

        fn get_schema_snapshot(&self, _prefix: &str) -> StorageResult<Option<SchemaSnapshot>> {
            unreachable!()
        }

        fn set_schema_snapshot(
            &mut self,
            _prefix: &str,
            _snapshot: &SchemaSnapshot,
        ) -> StorageResult<()> {
            unreachable!()
        }

        fn get_migration_log(&self, _prefix: &str) -> StorageResult<Option<Vec<AppliedStep>>> {
            unreachable!()
        }

        fn set_migration_log(&mut self, _prefix: &str, _log: &[AppliedStep]) -> StorageResult<()> {
            unreachable!()
        }
    }

    #[test]
    fn test_context_rename() {
        let mut storage = MemoryStorage {
            data: HashMap::new(),
        };
        let mut ctx = MigrationContext::new("p".into(), &mut storage);

        ctx.set("a", &100i32).unwrap();
        ctx.rename("a", "b").unwrap();

        assert_eq!(ctx.get::<i32>("b").unwrap(), Some(100));
        assert!(ctx.get::<i32>("a").unwrap().is_none());
    }

    #[test]
    fn test_context_transform() {
        let mut storage = MemoryStorage {
            data: HashMap::new(),
        };
        let mut ctx = MigrationContext::new("p".into(), &mut storage);

        ctx.set("v", &10i32).unwrap();
        ctx.transform::<i32, i32>("v", |v| Ok(v + 5)).unwrap();

        assert_eq!(ctx.get::<i32>("v").unwrap(), Some(15));
    }

    #[test]
    fn test_context_merge() {
        let mut storage = MemoryStorage {
            data: HashMap::new(),
        };
        let mut ctx = MigrationContext::new("p".into(), &mut storage);

        ctx.set("f", &"a".to_string()).unwrap();
        ctx.set("l", &"b".to_string()).unwrap();

        ctx.merge::<String, String, String>(("f", "l"), "res", |f, l| Ok(format!("{}{}", f, l)))
            .unwrap();

        assert_eq!(ctx.get::<String>("res").unwrap(), Some("ab".into()));
        assert!(ctx.get::<String>("f").unwrap().is_none());
        assert!(ctx.get::<String>("l").unwrap().is_none());
    }

    #[test]
    fn test_context_split() {
        let mut storage = MemoryStorage {
            data: HashMap::new(),
        };
        let mut ctx = MigrationContext::new("p".into(), &mut storage);

        ctx.set("full", &"a:b".to_string()).unwrap();

        ctx.split::<String, String, String>("full", ("p1", "p2"), |s| {
            let mut it = s.split(':');
            Ok((
                it.next().unwrap().to_string(),
                it.next().unwrap().to_string(),
            ))
        })
        .unwrap();

        assert_eq!(ctx.get::<String>("p1").unwrap(), Some("a".into()));
        assert_eq!(ctx.get::<String>("p2").unwrap(), Some("b".into()));
        assert!(ctx.get::<String>("full").unwrap().is_none());
    }

    #[test]
    fn test_global_access() {
        let mut storage = MemoryStorage {
            data: HashMap::new(),
        };
        let mut ctx = MigrationContext::new("scoped".into(), &mut storage);

        ctx.global_set("raw.key", &777u32).unwrap();

        assert!(ctx.get::<u32>("raw.key").unwrap().is_none());
        assert_eq!(ctx.global_get::<u32>("raw.key").unwrap(), Some(777));
    }
}
