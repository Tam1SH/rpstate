use amethystate::amethystate;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;
use std::error::Error;

#[amethystate]
pub struct Inherits {
    #[amestate(default = 5432u16)]
    pub port: u16,
}

#[amethystate(on_unreadable = Refuse)]
pub struct Insists {
    #[amestate(default = 5432u16)]
    pub port: u16,
}

#[amethystate(prefix = "lenient_root", on_unreadable = UseDefault)]
pub struct LenientRoot {
    #[amestate(nested)]
    pub db: Inherits,
}

#[amethystate(prefix = "strict_child", on_unreadable = UseDefault)]
pub struct StrictChild {
    #[amestate(nested)]
    pub db: Insists,
}

#[test]
fn a_nested_struct_inherits_what_the_one_holding_it_decided()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("nested_policy_inherits");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["lenient_root", "db", "port"], &"not a number".to_string())?;

    let state = LenientRoot::new_with(&store)?;

    assert_eq!(state.db().port().get(), 5432);
    assert!(state.db().port().try_get().is_err());

    Ok(())
}

#[test]
fn what_the_nested_struct_declared_wins() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("nested_policy_insists");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["strict_child", "db", "port"], &"not a number".to_string())?;

    assert!(StrictChild::new_with(&store).is_err());

    Ok(())
}

#[test]
fn a_nested_struct_opens_normally_when_nothing_is_wrong() -> Result<(), Box<dyn Error + Send + Sync>>
{
    let path = TempPath::new("nested_policy_ordinary");
    let store = StoreBuilder::new(path.path()).build()?;

    let state = LenientRoot::new_with(&store)?;

    assert_eq!(state.db().port().try_get()?, 5432);

    Ok(())
}
