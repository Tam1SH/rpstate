use amethystate::{StoreBuilder, amethystate};
use amethystate_core::test_utils::unique_path;

#[amethystate(prefix = "durable")]
pub struct Settings {
    #[amestate(default = 1)]
    pub port: u16,

    #[amestate(default = 0)]
    pub retries: u8,
}

#[amethystate(prefix = "durable_vol")]
pub struct Volatile {
    #[amestate(default = 0, volatile)]
    pub scratch: u8,
}

#[test]
fn a_volatile_field_is_already_durable() {
    let store = StoreBuilder::new(unique_path("durable_volatile"))
        .build()
        .unwrap();
    let state = Volatile::new_with(&store).unwrap();

    state.scratch().set_durable(3).unwrap();
    futures::executor::block_on(state.scratch().set_durable_async(4).unwrap()).unwrap();

    assert_eq!(
        state.scratch().get(),
        4,
        "nothing to commit, so nothing to wait for"
    );
}

#[test]
fn the_async_write_lands_without_awaiting_it() {
    let store = StoreBuilder::new(unique_path("durable_visible"))
        .debounce(60_000)
        .build()
        .unwrap();
    let state = Settings::new_with(&store).unwrap();

    let commit = state.retries().set_durable_async(7).unwrap();

    assert_eq!(
        state.retries().get(),
        7,
        "the value is live before the commit is awaited"
    );

    futures::executor::block_on(commit).unwrap();
}

/// Only a text backend lets the file be read while the store still holds it,
/// which is what makes the commit observable without closing anything. The
/// flush itself is one code path shared by every backend.
#[cfg(feature = "json")]
mod on_disk {
    use super::*;

    fn contents(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn set_durable_commits_before_it_returns() {
        let path = unique_path("durable_blocking");
        let store = StoreBuilder::new(&path).debounce(60_000).build().unwrap();
        let state = Settings::new_with(&store).unwrap();

        state.port().set(8080).unwrap();
        assert!(
            !contents(&path).contains("8080"),
            "a plain set leaves it buffered, and the debouncer is a minute away"
        );

        state.port().set_durable(9090).unwrap();
        assert!(
            contents(&path).contains("\"port\": 9090"),
            "set_durable committed before returning, with no close and no timer"
        );
    }

    #[test]
    fn set_durable_async_commits_before_it_resolves() {
        let path = unique_path("durable_async");
        let store = StoreBuilder::new(&path).debounce(60_000).build().unwrap();
        let state = Settings::new_with(&store).unwrap();

        let commit = state.retries().set_durable_async(7).unwrap();
        futures::executor::block_on(commit).unwrap();

        let found = contents(&path);
        assert!(
            found.contains("\"retries\": 7"),
            "resolving the future means the value is on disk, got: {found}"
        );
    }
}
