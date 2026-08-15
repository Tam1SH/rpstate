use amethystate::migration::set::MigrationSet;
use amethystate::{Store, StoreConfig, amethystate};
use amethystate_core::test_utils::unique_path;
#[amethystate(as_root)]
pub struct AppConfig {
    #[amestate(default = "legacy".to_string())]
    pub name: String,

    #[amestate(default = false)]
    pub comfy: bool,
}

#[test]
fn test_as_root_global_namespace() {
    let path = unique_path("as_root_test");
    let (store, _) = Store::open(StoreConfig::new(&path), MigrationSet::default()).unwrap();

    let config = AppConfig::new_with(&store).unwrap();

    assert_eq!(
        store.get::<String>("name").unwrap(),
        Some("legacy".to_string())
    );
    assert_eq!(store.get::<bool>("comfy").unwrap(), Some(false));

    config.name().set("updated_name".to_string()).unwrap();
    config.comfy().set(true).unwrap();

    assert_eq!(
        store.get::<String>("name").unwrap(),
        Some("updated_name".to_string())
    );
    assert_eq!(store.get::<bool>("comfy").unwrap(), Some(true));
}
