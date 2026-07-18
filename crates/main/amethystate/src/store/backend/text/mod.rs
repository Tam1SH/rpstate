pub mod document;
pub mod error;
mod inspector;
#[cfg(feature = "json")]
pub mod json;
pub mod migration;
#[cfg(feature = "ron")]
pub mod ron;
pub mod store;
#[cfg(feature = "toml")]
pub mod toml;

pub use document::TextDocument;
pub use error::TextStoreError;
pub use store::TextStore;

#[cfg(feature = "json")]
pub use json::JsonStore;

#[cfg(feature = "toml")]
pub use toml::TomlStore;

#[cfg(feature = "ron")]
pub use ron::RonStore;

#[macro_export]
macro_rules! define_store_test_suite {
    ($store_type:ident, $ext:expr, $watch_set_false:expr, $watch_set_true:expr, $watch_delete_empty:expr) => {
        #[cfg(test)]
        mod store_tests {
            use super::*;
            use parking_lot::Mutex;
            use std::path::PathBuf;
            use std::sync::Arc;
            use std::time::{Duration, SystemTime, UNIX_EPOCH};
            use $crate::store::config::StoreConfig;
            use $crate::store::{Store, StoreEvent, StoreOp, SubscriptionKind};

            fn unique_test_path(suffix: &str) -> PathBuf {
                let nanos = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("time is after epoch")
                    .as_nanos();
                std::env::temp_dir().join(format!(
                    "amethystate-{}-{suffix}-{nanos}.{}",
                    stringify!($store_type),
                    $ext
                ))
            }

            fn make_store(suffix: &str) -> $store_type {
                $store_type::open(
                    StoreConfig::new(unique_test_path(suffix)),
                    Default::default(),
                )
                .unwrap()
                .0
            }

            fn open_store_at(path: PathBuf) -> $store_type {
                $store_type::open(StoreConfig::new(path), Default::default())
                    .unwrap()
                    .0
            }

            #[test]
            fn set_get_delete_roundtrip() {
                let store = make_store("roundtrip");

                store
                    .set("ui.theme.dark", &true)
                    .expect("set should succeed");
                assert_eq!(store.get::<bool>("ui.theme.dark").unwrap(), Some(true));

                store
                    .delete("ui.theme.dark")
                    .expect("delete should succeed");
                assert_eq!(store.get::<bool>("ui.theme.dark").unwrap(), None);
            }

            #[test]
            fn subscriptions_any_exact_prefix_fire() {
                let store = make_store("subscriptions");

                let any_hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
                let exact_hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
                let prefix_hits: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));

                let cap = any_hits.clone();
                store.subscribe(
                    SubscriptionKind::Any,
                    Arc::new(move |evt| {
                        cap.lock().push(evt.path.to_string());
                    }),
                );

                let cap = exact_hits.clone();
                store.subscribe(
                    SubscriptionKind::ExactPath(Arc::from("ui.theme.dark")),
                    Arc::new(move |evt| {
                        cap.lock().push(evt.path.to_string());
                    }),
                );

                let cap = prefix_hits.clone();
                store.subscribe(
                    SubscriptionKind::Prefix(Arc::from("ui.theme")),
                    Arc::new(move |evt| {
                        cap.lock().push(evt.path.to_string());
                    }),
                );

                store
                    .set("ui.theme.dark", &true)
                    .expect("set should succeed");
                store
                    .set("ui.layout.sidebar_width", &260u64)
                    .expect("set should succeed");

                assert_eq!(any_hits.lock().len(), 2);
                assert_eq!(exact_hits.lock().as_slice(), ["ui.theme.dark"]);
                assert_eq!(prefix_hits.lock().as_slice(), ["ui.theme.dark"]);
            }

            #[test]
            fn unsubscribe_stops_callbacks() {
                let store = make_store("unsubscribe");

                let hit_count = Arc::new(Mutex::new(0usize));
                let cap = hit_count.clone();
                let id = store.subscribe(
                    SubscriptionKind::Any,
                    Arc::new(move |_| {
                        *cap.lock() += 1;
                    }),
                );

                store
                    .set("ui.theme.dark", &true)
                    .expect("set should succeed");
                store.unsubscribe(id);
                store
                    .set("ui.theme.dark", &false)
                    .expect("set should succeed");

                assert_eq!(*hit_count.lock(), 1);
            }

            #[test]
            fn file_watch_emits_set_for_external_change() {
                let path = unique_test_path("watch-set");
                std::fs::write(&path, $watch_set_false).expect("seed file should be written");

                let store = open_store_at(path.clone());
                let (tx, rx) = std::sync::mpsc::channel::<StoreEvent>();

                store.subscribe(
                    SubscriptionKind::ExactPath(Arc::from("ui.theme.dark")),
                    Arc::new(move |evt| {
                        let _ = tx.send(evt.clone());
                    }),
                );

                std::fs::write(&path, $watch_set_true).expect("updated file should be written");

                let event = rx
                    .recv_timeout(Duration::from_secs(3))
                    .expect("watcher should emit set event");

                assert_eq!(&*event.path, "ui.theme.dark");
                assert_eq!(event.op, StoreOp::Set);
                let old_val: bool = store.decode(&event.old.as_ref().unwrap()).unwrap();
                let new_val: bool = store.decode(&event.new.as_ref().unwrap()).unwrap();
                assert_eq!(old_val, false);
                assert_eq!(new_val, true);
            }

            #[test]
            fn file_watch_emits_delete_for_external_removal() {
                let path = unique_test_path("watch-delete");
                std::fs::write(&path, $watch_set_true).expect("seed file should be written");

                let store = open_store_at(path.clone());
                let (tx, rx) = std::sync::mpsc::channel::<StoreEvent>();

                store.subscribe(
                    SubscriptionKind::ExactPath(Arc::from("ui.theme.dark")),
                    Arc::new(move |evt| {
                        let _ = tx.send(evt.clone());
                    }),
                );

                std::fs::write(&path, $watch_delete_empty).expect("updated file should be written");

                let event = rx
                    .recv_timeout(Duration::from_secs(3))
                    .expect("watcher should emit delete event");

                assert_eq!(&*event.path, "ui.theme.dark");
                assert_eq!(event.op, StoreOp::Delete);
                let old_val: bool = store.decode(&event.old.as_ref().unwrap()).unwrap();
                assert_eq!(old_val, true);
                assert_eq!(event.new, None);
            }

            #[test]
            fn save_now_and_persist() {
                let path = unique_test_path("save_now");
                let store = open_store_at(path.clone());

                store.set("app.version", &"1.0.0".to_string()).unwrap();
                store.set("app.debug", &true).unwrap();

                if path.exists() {
                    std::fs::remove_file(&path).unwrap();
                }

                store.save_now().unwrap();

                assert!(path.exists());
                let content = std::fs::read_to_string(&path).unwrap();
                assert!(content.contains("1.0.0"));
            }

            #[test]
            fn test_is_initialized_false_on_fresh_store() {
                let store = make_store("init_fresh");
                assert!(!store.is_initialized("settings").unwrap());
            }

            #[test]
            fn test_mark_and_is_initialized() {
                let store = make_store("init_mark");
                assert!(!store.is_initialized("settings").unwrap());
                store.mark_initialized("settings").unwrap();
                assert!(store.is_initialized("settings").unwrap());
            }

            #[test]
            fn test_initialized_namespaces_are_independent() {
                let store = make_store("init_namespaces");
                store.mark_initialized("settings").unwrap();
                assert!(store.is_initialized("settings").unwrap());
                assert!(!store.is_initialized("other").unwrap());
            }

            #[test]
            fn test_init_key_does_not_appear_in_scan_prefix() {
                let store = make_store("init_scan");
                store.mark_initialized("settings").unwrap();
                store
                    .set("settings.host", &"localhost".to_string())
                    .unwrap();

                let entries = store.scan_prefix("settings").unwrap();
                assert!(
                    entries.iter().all(|(k, _)| !k.contains("__init")),
                    "init key should not appear in scan_prefix results"
                );
            }
        }
    };
}
