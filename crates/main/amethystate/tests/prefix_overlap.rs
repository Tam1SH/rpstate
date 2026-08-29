//! Two owners cannot have one place on disk.
//!
//! A path can be arrived at more than one way - a dotted `key`, a dotted
//! `prefix`, a struct rooted where another stores a value - and until the
//! claims table nothing compared them. The last writer won, silently, and the
//! damage only showed on the next start when the signals had to come off the
//! disk again.

use amethystate::store::builder::StoreBuilder;
use amethystate::store::owners::Claimed;
use amethystate_core::facts::all;
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

/// A dotted `key` under one prefix and a plain field under a dotted `prefix`
/// compose to the same path. The second construction is refused, and the report
/// names both schemas rather than leaving the two to share a slot.
#[test]
fn two_structs_cannot_claim_the_same_stored_path() {
    let path = TempPath::new("prefix_overlap");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let _outer = Outer::new_with(&store).unwrap();
    let refused = Panels::new_with(&store).unwrap_err();

    let named: Vec<String> = all::<Claimed, _>(&refused)
        .map(|claim| claim.by.to_string())
        .collect();

    assert_eq!(named.len(), 2, "the report names both: {refused:?}");
    assert!(
        named.iter().any(|by| by.contains("Outer"))
            && named.iter().any(|by| by.contains("Panels")),
        "and names them by their schemas: {named:?}"
    );
}

/// Types that disagree used to surface as a decode failure while the second
/// struct was being built - a codec error standing in for a name collision.
/// It is the same refusal as when they agree, because the claim is about the
/// path and not about what is stored at it.
#[test]
fn an_overlap_between_different_types_is_reported_as_an_overlap() {
    let path = TempPath::new("prefix_overlap_typed");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let _outer = TypedOuter::new_with(&store).unwrap();
    let refused = TypedPanels::new_with(&store).unwrap_err();

    let rendered = format!("{refused:?}");
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

/// One struct stores a value at `root.b`, another roots itself there. A field
/// owns what is under it - that is the inside of its value - so the branch is
/// refused rather than left to put a level where a leaf already is.
#[test]
fn a_prefix_may_not_land_on_another_structs_field() {
    let path = TempPath::new("root_is_a_leaf");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    let _root = Root::new_with(&store).unwrap();
    let refused = Branch::new_with(&store).unwrap_err();

    let claims: Vec<String> = all::<Claimed, _>(&refused)
        .map(|claim| claim.path.as_str().to_string())
        .collect();

    assert!(
        claims.iter().any(|p| p == "root.b") && claims.iter().any(|p| p == "root.b.x"),
        "the report names the leaf and the branch that wanted to sit under it: {claims:?}"
    );
}

/// A map owns the level below it and nothing further, so a key two levels down
/// is somebody else's. Reading it as an entry gave the shallower name the
/// deeper value's bytes, and a second key under the same name displaced the
/// first by the scan's order. Now the map refuses to open and says which key.
#[test]
fn a_map_will_not_open_over_keys_deeper_than_its_entries() {
    let path = TempPath::new("map_swallows_below");
    let store = StoreBuilder::new(path.path()).build().unwrap();

    store.set(["widths", "left", "px"], &800u32).unwrap();
    store.set(["widths", "left", "pct"], &50u32).unwrap();
    store.save_now().unwrap();

    let refused = store
        .kv()
        .map::<String, u32>("widths")
        .expect_err("the map cannot be read over keys that are not its entries");

    let rendered = format!("{refused:?}");
    assert!(
        rendered.contains("widths.left.p"),
        "the report names the key that is not an entry: {rendered}"
    );
    assert!(
        rendered.contains("a map owns the level below it"),
        "and says why it is not one: {rendered}"
    );
}
