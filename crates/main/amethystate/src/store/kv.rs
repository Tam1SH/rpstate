use crate::migration::types::AmeType;
use crate::observability::SchemaEntry;
use crate::observability::{register_instance, resolve_field};
use crate::reactive::error::{WriteError, WriteResult};
use crate::store::Durable;
use crate::store::{StorageResult, StoreBackend, field_with_path, reactive_map_with_path_only};
use crate::{ReactiveCell, ReactiveMap, Store, WritableMode};
use crate::{ReactiveMapKey, ReactiveMapValue};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Reactive values addressed by path, without declaring a struct.
///
/// For values whose set is not known at compile time, or where a schema is more
/// ceremony than the job is worth. Nothing here is versioned or migrated, and
/// drift is not tracked - that is what the typed structs are for.
///
/// What comes back is an ordinary [`ReactiveCell`] or [`ReactiveMap`], so
/// subscriptions, local delivery and pipelines work exactly as they do for
/// declared fields. Only the addressing differs.
pub struct Kv {
    store: Store,
    instance_id: Uuid,
    prefix: Option<Arc<str>>,
}

impl Kv {
    pub(crate) fn new(store: Store) -> Self {
        let instance_id = Uuid::new_v4();
        register_instance(instance_id, "amethystate::Kv");

        Self {
            store,
            instance_id,
            prefix: None,
        }
    }

    /// A handle on everything under `name`.
    ///
    /// Nesting is spelled here rather than punched into a string, so a name
    /// stays a name: `namespace("ui")` then `set("dark.mode", ..)` addresses one
    /// value called `dark.mode`, where a single `"ui.dark.mode"` would have been
    /// three levels. A name carrying the separator is refused for the same
    /// reason.
    ///
    /// Writes through a namespace carry the same provenance as the handle it
    /// came from - it is a view on the same `Kv`, not a second writer.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let ui = store.kv().namespace("ui").unwrap();
    ///
    /// ui.set("width", &1280u32).unwrap();
    /// assert_eq!(ui.get::<u32>("width").unwrap(), Some(1280));
    ///
    /// // The same value, spelled out from the root.
    /// assert_eq!(store.kv().namespace("ui").unwrap().get::<u32>("width").unwrap(), Some(1280));
    ///
    /// // Namespaces nest.
    /// let panels = ui.namespace("panels").unwrap();
    /// panels.set("left", &true).unwrap();
    /// assert_eq!(ui.namespace("panels").unwrap().get::<bool>("left").unwrap(), Some(true));
    /// ```
    pub fn namespace(&self, name: &str) -> WriteResult<Self> {
        Ok(Self {
            store: self.store.clone(),
            instance_id: self.instance_id,
            prefix: Some(Arc::from(self.resolve(name)?)),
        })
    }

    /// Where this handle is rooted, or `None` at the top.
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    fn resolve(&self, name: &str) -> WriteResult<String> {
        if name.contains('.') {
            return Err(WriteError::SeparatorInName {
                name: name.to_string(),
            });
        }

        Ok(match &self.prefix {
            Some(prefix) => format!("{prefix}.{name}"),
            None => name.to_string(),
        })
    }

    /// Reads a value, or `None` if the path holds nothing.
    ///
    /// Raw: the type is whatever you ask for here, and nothing remembers it.
    /// [`Kv::cell`] does, and refuses a second type for the same path.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// assert_eq!(kv.get::<u32>("width").unwrap(), None);
    /// kv.set("width", &1280u32).unwrap();
    /// assert_eq!(kv.get::<u32>("width").unwrap(), Some(1280));
    /// ```
    pub fn get<T: AmeType + DeserializeOwned>(&self, name: &str) -> WriteResult<Option<T>> {
        Ok(self.store.get(&self.resolve(name)?)?)
    }

    /// The same writes, each returning only once the change is on disk.
    pub fn durable(&self) -> Durable<'_, Self> {
        Durable(self)
    }

    /// Writes a value at `path`, creating it or replacing what was there.
    ///
    /// Returns only once the change is on disk rather than buffered.
    /// Writes a value at `path`, creating it or replacing what was there.
    ///
    /// Unlike [`ReactiveMap::update`] this has no strict variant: `Kv` is
    /// addressed by path and has no notion of a key that must already exist.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// kv.set("theme", &"dark".to_string()).unwrap();
    /// assert_eq!(kv.get::<String>("theme").unwrap(), Some("dark".into()));
    ///
    /// // A name is a name: nesting is asked for, never punctuated into one.
    /// assert!(kv.set("ui.theme", &"dark".to_string()).is_err());
    /// ```
    pub fn set<T: AmeType + Serialize>(&self, name: &str, value: &T) -> WriteResult<()> {
        let path = self.resolve(name)?;
        self.guard(&path)?;
        self.store
            .set_with_source(&path, value, Some(self.instance_id))?;
        Ok(())
    }

    /// Drops whatever is at `path`.
    ///
    /// Returns only once the change is on disk rather than buffered.
    /// Drops whatever is at `path`. Removing an absent path is not an error.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// kv.set("theme", &"dark".to_string()).unwrap();
    /// kv.remove("theme").unwrap();
    /// assert_eq!(kv.get::<String>("theme").unwrap(), None);
    ///
    /// // Again, on a name that now holds nothing.
    /// kv.remove("theme").unwrap();
    /// ```
    pub fn remove(&self, name: &str) -> WriteResult<()> {
        let path = self.resolve(name)?;
        self.guard(&path)?;
        self.store
            .delete_with_source(&path, Some(self.instance_id))?;
        Ok(())
    }

    /// Every path under `prefix`, sorted, without reading the values.
    ///
    /// The paths come back whole, prefix included - not as the remainder
    /// after it.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// let ui = kv.namespace("ui").unwrap();
    /// ui.set("theme", &"dark".to_string()).unwrap();
    /// ui.set("width", &1280u32).unwrap();
    /// kv.namespace("net").unwrap().set("port", &8080u16).unwrap();
    ///
    /// assert_eq!(ui.keys().unwrap(), ["ui.theme", "ui.width"]);
    /// ```
    pub fn keys(&self) -> StorageResult<Vec<String>> {
        match &self.prefix {
            Some(prefix) => self.store.scan_keys(prefix),
            None => self.store.scan_keys(""),
        }
    }

    /// A reactive cell over one path, seeded with `default` if the path is
    /// empty.
    ///
    /// The type is remembered, so asking for the same path as two different
    /// types fails rather than handing back garbage.
    ///
    /// The cell owns the field behind it, since nothing else holds one: the
    /// field exists because the cell was asked for. So it stays readable and
    /// writable for as long as it is held - and keeps the store open for that
    /// long - where [`Field::cell`](crate::Field::cell) is a view that empties
    /// when its field is dropped.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// let theme = kv.cell("theme", "dark".to_string()).unwrap();
    /// assert_eq!(theme.get(), Some("dark".to_string()));
    ///
    /// theme.set("light".to_string()).unwrap();
    /// assert_eq!(kv.get::<String>("theme").unwrap(), Some("light".into()));
    ///
    /// // The path is now typed, and a second type for it is refused.
    /// assert!(kv.cell("theme", 0u32).is_err());
    /// ```
    pub fn cell<T>(&self, name: &str, default: T) -> WriteResult<ReactiveCell<T>>
    where
        T: AmeType + Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
    {
        let path = self.resolve(name)?;
        self.guard(&path)?;
        self.check_type::<T>(&path)?;

        let field = field_with_path::<T, WritableMode>(
            &self.store,
            Arc::from(path.as_str()),
            default,
            self.instance_id,
        )?;

        let cell = field.cell();
        Ok(cell.owning(Arc::new(field)))
    }

    /// A reactive map under `name`, for a key set that is not known up front.
    ///
    /// Everything a declared `ReactiveMap` field can do, without declaring a
    /// struct - subscriptions, interceptors and durable writes included.
    ///
    /// ```
    /// # use amethystate::StoreBuilder;
    /// # let path = amethystate_core::test_utils::TempPath::new("doc");
    /// # let store = StoreBuilder::new(&*path).build().unwrap();
    /// let kv = store.kv();
    ///
    /// let widths = kv.map::<String, u64>("columns").unwrap();
    /// widths.insert("cpu".into(), &120).unwrap();
    ///
    /// // A map is a namespace with entries in it.
    /// let columns = kv.namespace("columns").unwrap();
    /// assert_eq!(columns.get::<u64>("cpu").unwrap(), Some(120));
    /// ```
    pub fn map<K, V>(&self, name: &str) -> WriteResult<ReactiveMap<K, V, WritableMode>>
    where
        K: ReactiveMapKey,
        V: AmeType + ReactiveMapValue,
    {
        let path = self.resolve(name)?;
        self.guard(&path)?;
        self.check_type::<V>(&path)?;

        Ok(reactive_map_with_path_only(
            &self.store,
            Arc::from(path.as_str()),
            HashMap::new(),
            self.instance_id,
        )?)
    }

    /// Refuses paths a declared struct owns.
    ///
    /// Writing a `String` where a `u16` is declared does not merely store the
    /// wrong thing: the field's subscription fails to decode and keeps its old
    /// value, and the next startup fails outright when it reads the path back.
    fn guard(&self, path: &str) -> WriteResult<()> {
        for entry in inventory::iter::<SchemaEntry> {
            let Some(prefix) = entry.prefix else {
                continue;
            };

            if path == prefix || path.starts_with(&format!("{}.", prefix)) {
                return Err(WriteError::SchemaOwned {
                    path: path.to_string(),
                    prefix: prefix.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Refuses a path already claimed as another type in this run.
    ///
    /// Only what this process has built is known, so this catches a mistake
    /// rather than guaranteeing a type. A path written by an earlier run is not
    /// checked, and neither is the raw [`Kv::set`].
    fn check_type<T: AmeType + 'static>(&self, path: &str) -> WriteResult<()> {
        let wanted = T::TYPE_NAME;

        match resolve_field(path) {
            Some(meta) if meta.value_type_name != wanted => Err(WriteError::TypeMismatch {
                path: path.to_string(),
                known: meta.value_type_name.to_string(),
                asked: wanted.to_string(),
            }),
            _ => Ok(()),
        }
    }
}

impl Durable<'_, Kv> {
    /// Writes a value at `path`, creating it or replacing what was there.
    ///
    /// Returns only once it is on disk rather than buffered.
    pub fn set<T: AmeType + Serialize>(&self, name: &str, value: &T) -> WriteResult<()> {
        self.0.set(name, value)?;
        self.0.store.flush_prefix(&self.0.resolve(name)?)?;
        Ok(())
    }

    /// Writes a value at `path`, creating it or replacing what was there.
    ///
    /// Resolves once the change is on disk rather than buffered.
    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn set_async<T: AmeType + Serialize>(
        &self,
        name: &str,
        value: &T,
    ) -> WriteResult<()> {
        self.0.set(name, value)?;
        self.0.store.flush_async().await?;
        Ok(())
    }

    /// Drops whatever is at `path`.
    ///
    /// Returns only once the removal is on disk rather than buffered.
    pub fn remove(&self, name: &str) -> WriteResult<()> {
        self.0.remove(name)?;
        self.0.store.flush_prefix(&self.0.resolve(name)?)?;
        Ok(())
    }

    /// Drops whatever is at `path`.
    ///
    /// Resolves once the change is on disk rather than buffered.
    /// Like every future, this does nothing until awaited - the write
    /// included. See [`Durable::set_async`].
    pub async fn remove_async(&self, path: &str) -> WriteResult<()> {
        self.0.remove(path)?;
        self.0.store.flush_async().await?;
        Ok(())
    }
}
