use amethystate::store::builder::{Backend, StoreBuilder};
use amethystate_core::test_utils::TempPath;
use serde::{Deserialize, Serialize};

mod common;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
enum Mode {
    Off,
    On(u8),
    Named { level: u8 },
}

fn shapes() -> [(&'static str, Mode); 3] {
    [
        ("unit variant", Mode::Off),
        ("tuple variant", Mode::On(3)),
        ("struct variant", Mode::Named { level: 7 }),
    ]
}

fn round_trips(backend: Backend) {
    for (label, value) in shapes() {
        let path = TempPath::new("enum_round_trip");
        let store = StoreBuilder::new(path.path())
            .backend(backend)
            .build()
            .unwrap();

        store.set(["probe", "mode"], &value).unwrap();

        let read = store
            .get::<Mode>(["probe", "mode"])
            .unwrap_or_else(|e| panic!("{backend:?} {label}: {e:?}"));

        assert_eq!(read, Some(value), "{backend:?} {label}");
    }
}

#[test]
fn an_enum_survives_a_round_trip() {
    for backend in common::enabled_backends() {
        if common::engine_name(backend) == "ron" {
            continue;
        }
        round_trips(backend);
    }
}

#[cfg(feature = "ron")]
#[test]
#[ignore = "known: ron's node type cannot hold an enum, so the variant is \
            dropped at the write - see TODO.md"]
fn ron_carries_an_enum_too() {
    round_trips(Backend::Ron);
}
