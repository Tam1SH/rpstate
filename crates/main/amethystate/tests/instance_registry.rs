use amethystate::observability::resolve_instance;
use amethystate::{StoreBuilder, amethystate};
use amethystate_core::test_utils::unique_path;
use uuid::Uuid;

#[amethystate(prefix = "registry")]
pub struct Tracked {
    #[amestate(default = 0)]
    pub port: u16,
}

fn store(tag: &str) -> impl amethystate::Store {
    StoreBuilder::new(unique_path(tag)).build().unwrap()
}

#[test]
fn an_instance_leaves_the_registry_when_it_drops() {
    let s = store("drop");
    let id = Uuid::new_v4();

    {
        let _state = Tracked::new_with_id(&s, id).unwrap();
        assert!(resolve_instance(id).is_some(), "registered while alive");
    }

    assert!(resolve_instance(id).is_none(), "dropped from the registry");
}

#[test]
fn a_clone_keeps_the_instance_alive() {
    let s = store("clone");
    let id = Uuid::new_v4();

    let state = Tracked::new_with_id(&s, id).unwrap();
    let clone = state.clone();

    drop(state);
    assert!(
        resolve_instance(id).is_some(),
        "the surviving clone still resolves"
    );

    drop(clone);
    assert!(resolve_instance(id).is_none(), "the last clone released it");
}

#[test]
fn repeated_construction_does_not_accumulate() {
    let s = store("churn");
    let ids: Vec<Uuid> = (0..64).map(|_| Uuid::new_v4()).collect();

    for id in &ids {
        let _state = Tracked::new_with_id(&s, *id).unwrap();
    }

    let still_registered = ids.iter().filter(|id| resolve_instance(**id).is_some());
    assert_eq!(still_registered.count(), 0, "nothing left behind");
}
