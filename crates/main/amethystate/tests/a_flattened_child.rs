use amethystate::amethystate;
use amethystate::store::builder::{Backend, StoreBuilder};
use amethystate_core::test_utils::unique_path;
use amethystate_test_macros::backends;

#[amethystate]
pub struct Window {
    #[amestate(default = 800u32)]
    pub width: u32,
}

#[amethystate]
pub struct Fonts {
    #[amestate(default = 14u32)]
    pub size: u32,
}

#[amethystate(prefix = "editor")]
pub struct Editor {
    #[serde(flatten)]
    #[amestate(nested)]
    pub window: Window,

    #[amestate(nested)]
    pub fonts: Fonts,
}

#[backends(all)]
fn a_flattened_child_gives_up_its_segment(backend: Backend) {
    let path = unique_path("a_flattened_child");
    let store = StoreBuilder::new(&path).backend(backend).build().unwrap();

    let editor = Editor::new_with(&store).unwrap();

    assert_eq!(store.get::<u32>(["editor", "width"]).unwrap(), Some(800));
    assert_eq!(
        store.get::<u32>(["editor", "window", "width"]).unwrap(),
        None
    );
    assert_eq!(
        store.get::<u32>(["editor", "fonts", "size"]).unwrap(),
        Some(14)
    );

    editor.window.width().set(1024).unwrap();
    store.save_now().unwrap();

    assert_eq!(store.get::<u32>(["editor", "width"]).unwrap(), Some(1024));

    store.close().unwrap();
}

#[amethystate(prefix = "deep")]
pub struct Outer {
    #[serde(flatten)]
    #[amestate(nested)]
    pub middle: Middle,
}

#[amethystate]
pub struct Middle {
    #[serde(flatten)]
    #[amestate(nested)]
    pub window: Window,
}

#[backends(all)]
fn flattening_passes_through_more_than_one_level(backend: Backend) {
    let path = unique_path("a_flattened_grandchild");
    let store = StoreBuilder::new(&path).backend(backend).build().unwrap();

    let _outer = Outer::new_with(&store).unwrap();

    assert_eq!(store.get::<u32>(["deep", "width"]).unwrap(), Some(800));

    store.close().unwrap();
}
