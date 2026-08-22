use amethystate::store::builder::StoreBuilder;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;
use amethystate_macros::amethystate;

#[amethystate(prefix = "coll", version = 1)]
pub struct Outer {
    #[amestate(key = "panels.left.visible", default = true)]
    pub left_panel_visible: bool,
}

#[amethystate(prefix = "coll.panels", version = 1)]
pub struct Panels {
    #[amestate(key = "left.visible", default = true)]
    pub left_visible: bool,
}

#[amethystate(prefix = "typed", version = 1)]
pub struct TypedOuter {
    #[amestate(key = "panels.left.visible", default = true)]
    pub left_panel_visible: bool,
}

#[amethystate(prefix = "typed.panels", version = 1)]
pub struct TypedPanels {
    #[amestate(key = "left.visible", default = 0u32)]
    pub left_visible: u32,
}

/// A `key` override and a dotted `prefix` can name the same place on disk, and
/// nothing checks that they do not. With matching types the two structs share
/// one slot and the last writer wins.
#[test]
#[ignore = "known: overlapping stored paths are neither prevented nor reported - see TODO.md"]
fn two_structs_cannot_claim_the_same_stored_path() {
    let path = TempPath::new("prefix_overlap");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let outer = Outer::new_with(&store).unwrap();
    let panels = Panels::new_with(&store).unwrap();

    outer.left_panel_visible().set(false).unwrap();
    panels.left_visible().set(true).unwrap();

    assert!(
        !outer.left_panel_visible().get(),
        "the write through Panels landed on Outer's field"
    );
}

/// With types that disagree the overlap does surface, but as a decode failure
/// while the second struct is being built - nothing names the real problem.
#[test]
#[ignore = "known: overlapping stored paths are neither prevented nor reported - see TODO.md"]
fn an_overlap_between_different_types_is_reported_as_an_overlap() {
    let path = TempPath::new("prefix_overlap_typed");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let _outer = TypedOuter::new_with(&store).unwrap();
    let err = TypedPanels::new_with(&store).unwrap_err();

    let rendered = format!("{err:?}");
    assert!(
        rendered.contains("TypedOuter") && rendered.contains("TypedPanels"),
        "the report should name both schemas claiming `typed.panels.left.visible`, got: {rendered}"
    );
}

#[amethystate(prefix = "root", version = 1)]
pub struct Root {
    #[amestate(default = 1u32)]
    pub b: u32,
}

#[amethystate(prefix = "root.b", version = 1)]
pub struct Branch {
    #[amestate(default = 2u32)]
    pub x: u32,
}

/// One struct stores a value at `root.b`, another roots itself there. The leaf
/// and the branch want the same node.
#[test]
#[ignore = "known: overlapping stored paths are neither prevented nor reported - see TODO.md"]
fn a_prefix_may_not_land_on_another_structs_field() {
    let path = TempPath::new("root_is_a_leaf");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let root = Root::new_with(&store).unwrap();
    let branch = Branch::new_with(&store).unwrap();

    root.b().set(10).unwrap();
    branch.x().set(20).unwrap();

    store.flush_prefix(StorePath::root()).unwrap();

    assert_eq!(
        store.get::<u32>(["root", "b"]).unwrap(),
        Some(10),
        "the leaf on disk"
    );
    assert_eq!(
        store.get::<u32>(["root", "b", "x"]).unwrap(),
        Some(20),
        "the branch on disk"
    );
}
