use crate::{MigrationReport, Store, StoreBuilder};
use std::path::Path;
use std::sync::OnceLock;

static GLOBAL_STORE: OnceLock<Store> = OnceLock::new();

pub trait IntoGlobalStore {
    type Output;

    fn init_global(self) -> Self::Output;
}

impl IntoGlobalStore for StoreBuilder {
    type Output = MigrationReport;

    fn init_global(self) -> Self::Output {
        let (store, report) = self.build_with_report().unwrap_or_else(|err| {
            panic!(
                "amethystate: Failed to build global StoreBackend.\n\
                     Ensure the database path is writable and not locked by another process.\n\
                     Details: {err}"
            );
        });

        GLOBAL_STORE.set(store).unwrap_or_else(|_| {
            panic!(
                "amethystate: Global store is already initialized.\n\
                     Ensure `init_global` is called exactly once during application startup."
            );
        });

        report
    }
}

impl IntoGlobalStore for &str {
    type Output = MigrationReport;

    fn init_global(self) -> Self::Output {
        StoreBuilder::new(self).init_global()
    }
}

impl IntoGlobalStore for &Path {
    type Output = MigrationReport;

    fn init_global(self) -> Self::Output {
        StoreBuilder::new(self).init_global()
    }
}

pub fn init_global<T: IntoGlobalStore>(source: T) -> T::Output {
    source.init_global()
}

pub fn global_store() -> Store {
    GLOBAL_STORE.get().unwrap().clone()
}
