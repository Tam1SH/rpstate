use crate::Store;

pub fn unique_store(suffix: &str) -> Store {
    use crate::store::config::StoreConfig;
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("amethystate-test-{suffix}-{nanos}.db"));

    crate::store::builder::default_backend()
        .open_public(StoreConfig::new(path), Default::default())
        .unwrap()
        .0
}
