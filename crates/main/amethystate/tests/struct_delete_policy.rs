use amethystate::amethystate;
use amethystate::store::StoreBackend;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::path::StorePath;
use amethystate_core::test_utils::TempPath;
use std::error::Error;

#[amethystate(prefix = "resets", on_delete = UseDefault)]
pub struct Resets {
    #[amestate(default = 800u32)]
    pub width: u32,
}

#[amethystate(prefix = "holds")]
pub struct Holds {
    #[amestate(default = 800u32)]
    pub width: u32,
}

//@show a field that wants the default back when its key goes
#[amethystate(prefix = "mixed_delete")]
pub struct MixedDelete {
    #[amestate(default = 800u32)]
    pub width: u32,

    #[amestate(default = 600u32, on_delete = UseDefault)]
    pub height: u32,
}
//@show-end

#[test]
fn use_default_reports_the_declared_default_again() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("delete_policy_resets");
    let store = StoreBuilder::new(path.path()).build()?;
    let state = Resets::new_with(&store)?;

    state.width().set(1200)?;
    assert_eq!(state.width().get(), 1200);

    StoreBackend::delete(&store, &StorePath::from_segments(["resets", "width"]))?;

    assert_eq!(state.width().get(), 800);

    Ok(())
}

#[test]
fn a_deleted_key_goes_on_reporting_the_last_value() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("delete_policy_holds");
    let store = StoreBuilder::new(path.path()).build()?;
    let state = Holds::new_with(&store)?;

    state.width().set(1200)?;

    StoreBackend::delete(&store, &StorePath::from_segments(["holds", "width"]))?;

    assert_eq!(state.width().get(), 1200);
    assert_eq!(store.get::<u32>(["holds", "width"])?, None);

    Ok(())
}

#[test]
fn a_field_may_disagree_with_the_struct() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("delete_policy_mixed");
    let store = StoreBuilder::new(path.path()).build()?;
    let state = MixedDelete::new_with(&store)?;

    state.width().set(1200)?;
    state.height().set(900)?;

    StoreBackend::delete(&store, &StorePath::from_segments(["mixed_delete", "width"]))?;
    StoreBackend::delete(
        &store,
        &StorePath::from_segments(["mixed_delete", "height"]),
    )?;

    assert_eq!(state.width().get(), 1200);
    assert_eq!(state.height().get(), 600);

    Ok(())
}
