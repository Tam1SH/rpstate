//! A step that stays out of the linker's reach.
//!
//! `#[migrate]` registers through `inventory`, which collects at link time -
//! the step is found rather than named, and an application that would rather
//! hand its migrations over has nothing to hand. `#[migrate(explicit)]` leaves
//! a `const` named for the function instead, and `add_steps` takes it.
//!
//! The pair below is one fixture opened twice: the step is invisible to the
//! collector, and runs when it is passed in.

use amethystate::store::builder::StoreBuilder;
use amethystate::{AmeData, migrate};
use amethystate_core::test_utils::unique_path;
use amethystate_macros::amethystate;

mod v1 {
    use super::*;

    #[amethystate(prefix = "explicit_steps", version = 1)]
    pub struct Settings {
        #[amestate(default = 8080u16)]
        pub port: u16,
    }
}

#[amethystate(prefix = "explicit_steps", version = 2)]
pub struct Settings {
    #[amestate(default = 8080u16)]
    pub port: u16,

    #[amestate(default = "untouched".to_string())]
    pub host: String,
}

#[migrate(explicit)]
fn settings_v1_to_v2(
    old: AmeData<v1::Settings>,
) -> amethystate::MigrationResult<AmeData<Settings>> {
    Ok(AmeData::<Settings> {
        port: old.port,
        host: "the step ran".to_string(),
    })
}

fn a_store_at_v1(path: &std::path::Path) {
    let store = StoreBuilder::new(path).build().unwrap();
    let _v1 = v1::Settings::new_with(&store).unwrap();
    store.save_now().unwrap();
}

/// `build_with_report` is the entry that sweeps the binary for steps, and this
/// one is not in the sweep.
#[test]
fn an_explicit_step_is_not_collected_from_the_linker() {
    let path = unique_path("migration_explicit_uncollected");
    a_store_at_v1(&path);

    let (store, _report) = StoreBuilder::new(&path).build_with_report().unwrap();

    let settings = Settings::new_with(&store).unwrap();
    assert_eq!(
        settings.host().get(),
        "untouched",
        "the step was declared `explicit` and should not have been found"
    );
}

/// Named and handed over, it does what any other step does.
#[test]
fn an_explicit_step_runs_when_it_is_handed_over() {
    let path = unique_path("migration_explicit_handed_over");
    a_store_at_v1(&path);

    let (store, report) = StoreBuilder::new(&path)
        .migrations(|m| {
            m.add_steps(&[SETTINGS_V1_TO_V2]);
        })
        .build_with_report()
        .unwrap();

    assert!(!report.has_failures(), "{report:?}");

    let settings = Settings::new_with(&store).unwrap();
    assert_eq!(
        settings.host().get(),
        "the step ran",
        "the step was passed in and should have run"
    );
    assert_eq!(
        settings.port().get(),
        8080,
        "and carried the old value over"
    );
}
