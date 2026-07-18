use amethystate::amethystate;
use amethystate_core::test_utils::unique_path;

#[amethystate]
pub struct ColumnsSettings {
    #[amestate(default = 70u64)]
    pub default_width_px: u64,

    #[amestate(default = {
        "name": 200u64,
        "cpu": 90u64,
    })]
    pub widths_px: amethystate::ReactiveMap<String, u64>,
}

#[amethystate(prefix = "process")]
pub struct ProcessSettings {
    #[amestate(default = 1500u64)]
    pub scan_interval_ms: u64,

    #[amestate(nested)]
    pub columns: ColumnsSettings,
}

#[test]
fn reactive_map_inside_nested_struct_seeds_defaults() {
    let path = unique_path("nested_reactive_map");
    let store = amethystate::StoreBuilder::new(&path).build().unwrap();
    let settings = ProcessSettings::new_with(&store).unwrap();

    let widths = settings.columns().widths_px();
    assert_eq!(widths.get(&"name".to_string()).unwrap(), Some(200u64));
    assert_eq!(widths.get(&"cpu".to_string()).unwrap(), Some(90u64));
}

#[test]
fn reactive_map_inside_nested_struct_seeds_defaults_only_once() {
    let path = unique_path("nested_reactive_map_once");

    {
        let store = amethystate::StoreBuilder::new(&path).build().unwrap();
        let settings = ProcessSettings::new_with(&store).unwrap();
        settings
            .columns()
            .widths_px()
            .set("name".to_string(), &999u64)
            .unwrap();
    }

    {
        let store = amethystate::StoreBuilder::new(&path).build().unwrap();
        let settings = ProcessSettings::new_with(&store).unwrap();

        // Reopening must not re-seed the default over the user's edit.
        assert_eq!(
            settings
                .columns()
                .widths_px()
                .get(&"name".to_string())
                .unwrap(),
            Some(999u64)
        );
    }
}
