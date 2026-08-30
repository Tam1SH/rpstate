use amethystate::amethystate;
use amethystate::errors::StorageError;
use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[amethystate(prefix = "strict")]
pub struct Strict {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,
}

#[amethystate(prefix = "lenient", on_unreadable = UseDefault)]
pub struct Lenient {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "127.0.0.1".to_string())]
    pub host: String,
}

//@show a struct that opens over a value it cannot read
#[amethystate(prefix = "mixed", on_unreadable = UseDefault)]
pub struct Mixed {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "".to_string(), on_unreadable = Refuse)]
    pub licence: String,
}
//@show-end

#[test]
fn an_undecodable_change_leaves_the_last_value_alone() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_live");
    let store = StoreBuilder::new(path.path()).build()?;
    let state = Strict::new_with(&store)?;

    state.port().set(9090)?;

    let woken = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    let _sub = state.port().subscribe(move |_| {
        count.fetch_add(1, Ordering::Release);
    });

    store.set(["strict", "port"], &"not a number".to_string())?;

    assert_eq!(state.port().get(), 9090);
    assert!(state.port().try_get().is_err());
    assert_eq!(woken.load(Ordering::Acquire), 0);

    Ok(())
}

#[test]
fn a_change_that_decodes_is_delivered_again() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_live_recovers");
    let store = StoreBuilder::new(path.path()).build()?;
    let state = Strict::new_with(&store)?;

    store.set(["strict", "port"], &"not a number".to_string())?;
    assert!(state.port().try_get().is_err());

    store.set(["strict", "port"], &1234u16)?;

    assert_eq!(state.port().try_get()?, 1234);

    Ok(())
}

#[test]
fn a_field_may_demand_more_than_the_struct_promised() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_mixed_licence");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["mixed", "licence"], &7u32)?;

    assert!(Mixed::new_with(&store).is_err());

    Ok(())
}

#[test]
fn the_struct_rule_still_covers_the_fields_that_did_not_ask()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_mixed_port");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["mixed", "port"], &"not a number".to_string())?;

    let state = Mixed::new_with(&store)?;

    assert_eq!(state.port().get(), 8080);
    assert!(state.port().try_get().is_err());

    Ok(())
}

#[test]
fn a_struct_refuses_to_open_over_a_value_it_cannot_read() -> Result<(), Box<dyn Error + Send + Sync>>
{
    let path = TempPath::new("read_policy_strict");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["strict", "port"], &"not a number".to_string())?;

    let refused = Strict::new_with(&store).unwrap_err();

    assert_eq!(refused.current_context(), &StorageError::Read);

    Ok(())
}

#[test]
fn use_default_opens_and_the_field_says_the_store_disagrees()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_lenient");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["lenient", "port"], &"not a number".to_string())?;

    let state = Lenient::new_with(&store)?;

    assert_eq!(state.port().get(), 8080);
    assert!(state.port().try_get().is_err());

    assert_eq!(state.host().get(), "127.0.0.1");
    assert!(state.host().try_get().is_ok());

    Ok(())
}

#[test]
fn use_default_leaves_the_stored_value_where_it_is() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_untouched");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["lenient", "port"], &"not a number".to_string())?;
    let _state = Lenient::new_with(&store)?;

    assert_eq!(
        store.get::<String>(["lenient", "port"])?,
        Some("not a number".to_string())
    );

    Ok(())
}

#[test]
fn a_write_that_decodes_clears_the_complaint() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_recovers");
    let store = StoreBuilder::new(path.path()).build()?;

    store.set(["lenient", "port"], &"not a number".to_string())?;
    let state = Lenient::new_with(&store)?;

    assert!(state.port().try_get().is_err());

    state.port().set(9090)?;

    assert_eq!(state.port().try_get()?, 9090);

    Ok(())
}

#[test]
fn a_readable_store_is_untouched_by_the_policy() -> Result<(), Box<dyn Error + Send + Sync>> {
    let path = TempPath::new("read_policy_ordinary");
    let store = StoreBuilder::new(path.path()).build()?;

    let state = Lenient::new_with(&store)?;

    assert_eq!(state.port().try_get()?, 8080);
    assert_eq!(state.host().try_get()?, "127.0.0.1");

    Ok(())
}
