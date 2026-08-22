use amethystate::observability;
use amethystate::{StoreBuilder, amethystate};
use amethystate_core::test_utils::unique_path;
use tracing_test::traced_test;

#[amethystate(prefix = "obs")]
pub struct ObsState {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "localhost".to_string())]
    pub host: String,
}

#[test]
fn instance_registered_on_new() {
    let path = unique_path("obs_instance_reg");
    let store = StoreBuilder::new(&path).build().unwrap();
    let _state = ObsState::new_with(&store).unwrap();

    let port_meta = observability::resolve_field("obs.port")
        .expect("obs.port must be in schema registry after construction");

    assert!(
        port_meta.struct_type_name.contains("ObsState"),
        "struct_type_name should contain 'ObsState', got: {}",
        port_meta.struct_type_name
    );
}

#[test]
fn fields_registered_in_schema_registry() {
    let path = unique_path("obs_schema_reg");
    let store = StoreBuilder::new(&path).build().unwrap();
    let _state = ObsState::new_with(&store).unwrap();

    let port_meta =
        observability::resolve_field("obs.port").expect("obs.port must be in schema registry");
    assert_eq!(port_meta.field_name.as_ref(), "port");
    assert!(
        port_meta.struct_type_name.contains("ObsState"),
        "struct_type_name should reference ObsState, got: {}",
        port_meta.struct_type_name
    );
    assert!(
        port_meta.value_type_name.contains("u16"),
        "value_type_name should be u16, got: {}",
        port_meta.value_type_name
    );

    let host_meta =
        observability::resolve_field("obs.host").expect("obs.host must be in schema registry");
    assert_eq!(host_meta.field_name.as_ref(), "host");
    assert!(host_meta.value_type_name.contains("String"));
}

#[test]
#[traced_test]
fn field_set_emits_trace() {
    let path = unique_path("obs_write_trace");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();

    state.port().set(9090).unwrap();

    assert!(logs_contain("field write"), "expected 'field write' trace");
    assert!(
        logs_contain("obs.port"),
        "expected path 'obs.port' in trace"
    );
}

#[test]
#[traced_test]
fn field_set_trace_contains_source_name() {
    let path = unique_path("obs_source_name");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();

    state.host().set("example.com".to_string()).unwrap();

    assert!(
        logs_contain("ObsState"),
        "expected struct name 'ObsState' in write trace"
    );
}

#[test]
#[traced_test]
fn subscription_fire_emits_trace_with_location() {
    let path = unique_path("obs_sub_trace");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();

    let _sub = state.port().subscribe(|_| {});
    state.port().set(1234).unwrap();

    assert!(
        logs_contain("signal emit"),
        "expected 'signal emit' trace on subscription fire"
    );
    assert!(
        logs_contain("observability_tracing.rs"),
        "expected call-site file name in subscription trace"
    );
}

/// A subscription built through [`Watch`] carries the same location a direct
/// `subscribe` does. The builder is three calls deep, and each of them would
/// otherwise put its own line in this library where the caller's belongs.
#[test]
#[traced_test]
fn a_built_subscription_traces_the_call_site_and_not_the_builder() {
    let path = unique_path("obs_watch_trace");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();

    let _sub = state.port().subscription_with().register(|_| {});
    state.port().set(4321).unwrap();

    assert!(
        logs_contain("signal emit"),
        "the subscription never fired, so there is no location to check"
    );
    assert!(
        logs_contain("observability_tracing.rs"),
        "expected the call site in the trace"
    );
    assert!(
        !logs_contain("watch.rs"),
        "the trace blamed the builder instead of the caller"
    );
}

#[test]
#[traced_test]
fn named_subscription_appears_in_trace() {
    let path = unique_path("obs_named_sub");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();

    let _sub = state.port().subscribe(|_| {}).named("PortWatcher");
    state.port().set(5555).unwrap();

    assert!(
        logs_contain("PortWatcher"),
        "expected named subscription label 'PortWatcher' in trace"
    );
}

#[test]
#[traced_test]
fn forked_write_traces_with_source() {
    let path = unique_path("obs_fork_trace");
    let store = StoreBuilder::new(&path).build().unwrap();
    let state = ObsState::new_with(&store).unwrap();
    let fork = state.fork();

    fork.port().set(7777).unwrap();

    assert!(
        logs_contain("field write"),
        "expected trace from fork write"
    );
    assert!(logs_contain("obs.port"));
}
