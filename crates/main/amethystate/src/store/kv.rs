use crate::observability::SchemaEntry;
use crate::observability::{register_instance, resolve_field};
use crate::reactive::error::{WriteError, WriteResult};
use crate::store::{StorageResult, Store, field_with_path, reactive_map_with_path_only};
use crate::{ReactiveCell, ReactiveMap, WritableMode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;
use std::str::FromStr;
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
pub struct Kv<S: Store> {
    store: S,
    instance_id: Uuid,
}

impl<S: Store> Kv<S> {
    pub(crate) fn new(store: S) -> Self {
        let instance_id = Uuid::new_v4();
        register_instance(instance_id, "amethystate::Kv");

        Self { store, instance_id }
    }

    /// Reads a value, or `None` if the path holds nothing.
    ///
    /// Raw: the type is whatever you ask for here, and nothing remembers it.
    /// [`Kv::cell`] does, and refuses a second type for the same path.
    pub fn get<T: DeserializeOwned>(&self, path: &str) -> StorageResult<Option<T>> {
        self.store.get(path)
    }

    pub fn set<T: Serialize>(&self, path: &str, value: &T) -> WriteResult<()> {
        self.guard(path)?;
        self.store
            .set_with_source(path, value, Some(self.instance_id))?;
        Ok(())
    }

    pub fn remove(&self, path: &str) -> WriteResult<()> {
        self.guard(path)?;
        self.store
            .delete_with_source(path, Some(self.instance_id))?;
        Ok(())
    }

    /// The keys under `prefix`, sorted. Values are not read.
    pub fn keys(&self, prefix: &str) -> StorageResult<Vec<String>> {
        self.store.scan_keys(prefix)
    }

    /// A reactive cell over one path, seeded with `default` if the path is
    /// empty.
    ///
    /// The type is remembered, so asking for the same path as two different
    /// types fails rather than handing back garbage.
    pub fn cell<T>(&self, path: &str, default: T) -> WriteResult<ReactiveCell<T>>
    where
        T: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
    {
        self.guard(path)?;
        self.check_type::<T>(path)?;

        let field = field_with_path::<T, S, WritableMode>(
            &self.store,
            Arc::from(path),
            default,
            self.instance_id,
        )?;

        Ok(field.cell())
    }

    /// A reactive map over a prefix, for a key set that is not known up front.
    pub fn map<K, V>(&self, prefix: &str) -> WriteResult<ReactiveMap<K, V, S, WritableMode>>
    where
        K: FromStr + Display + Clone + Hash + Eq + Send + Sync + 'static,
        V: Serialize + DeserializeOwned + Default + Clone + Send + Sync + 'static,
    {
        self.guard(prefix)?;
        self.check_type::<V>(prefix)?;

        Ok(reactive_map_with_path_only(
            &self.store,
            Arc::from(prefix),
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
    fn check_type<T: 'static>(&self, path: &str) -> WriteResult<()> {
        let wanted = std::any::type_name::<T>();

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
