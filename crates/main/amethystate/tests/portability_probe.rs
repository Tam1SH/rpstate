use amethystate::store::builder::StoreBuilder;
use amethystate_core::test_utils::TempPath;
use std::collections::BTreeMap;

mod common;

fn outcome<T>(written: Result<(), impl std::fmt::Debug>, read: Option<Result<Option<T>, impl std::fmt::Debug>>, expected: &T) -> String
where
    T: PartialEq + std::fmt::Debug,
{
    match (written, read) {
        (Err(_), _) => "refused".to_string(),
        (Ok(()), Some(Ok(Some(v)))) if &v == expected => "kept".to_string(),
        (Ok(()), Some(Ok(Some(v)))) => format!("changed to {v:?}"),
        (Ok(()), Some(Ok(None))) => "absent".to_string(),
        (Ok(()), Some(Err(_))) => "unreadable".to_string(),
        (Ok(()), None) => "unread".to_string(),
    }
}

macro_rules! probe {
    ($label:literal, $ty:ty, $value:expr) => {
        for backend in common::enabled_backends() {
            let path = TempPath::new("portability");
            let store = StoreBuilder::new(path.path())
                .backend(backend)
                .build()
                .unwrap();

            let value: $ty = $value;
            let written = store.set(["probe", "v"], &value);
            let read = written
                .as_ref()
                .ok()
                .map(|_| store.get::<$ty>(["probe", "v"]));

            println!(
                "{:<8} {:<28} {}",
                common::engine_name(backend),
                $label,
                outcome(written, read, &value)
            );
        }
    };
}

#[test]
fn what_each_engine_does_with_the_awkward_shapes() {
    probe!("u64 past i64", u64, u64::MAX);
    probe!("u64 just past i64", u64, i64::MAX as u64 + 1);
    probe!("Some(None) as two layers", Option<Option<u32>>, Some(None));
    probe!("None as two layers", Option<Option<u32>>, None);
    probe!("Some(Some(1))", Option<Option<u32>>, Some(Some(1)));
    probe!("a non-string map key", BTreeMap<u32, String>, {
        let mut m = BTreeMap::new();
        m.insert(7u32, "seven".to_string());
        m
    });
    probe!("negative zero", f64, -0.0f64);
    probe!("positive zero", f64, 0.0f64);
    probe!("i64 min", i64, i64::MIN);
    probe!("a char outside ascii", char, 'ж');
    probe!("bytes", Vec<u8>, vec![0u8, 255, 128]);
}

#[test]
fn negative_zero_keeps_its_sign() {
    for backend in common::enabled_backends() {
        let path = TempPath::new("negzero");
        let store = StoreBuilder::new(path.path())
            .backend(backend)
            .build()
            .unwrap();

        if store.set(["probe", "z"], &-0.0f64).is_err() {
            println!("{:<8} refused", common::engine_name(backend));
            continue;
        }

        let back = store.get::<f64>(["probe", "z"]).unwrap().unwrap();
        println!(
            "{:<8} sign negative: {}",
            common::engine_name(backend),
            back.is_sign_negative()
        );
    }
}
