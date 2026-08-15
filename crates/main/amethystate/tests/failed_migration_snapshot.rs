use amethystate::store::builder::StoreBuilder;
use amethystate::{AmeData, migrate};
use amethystate_core::test_utils::unique_path;
use amethystate_macros::amethystate;

mod v1 {
    use super::*;
    #[amethystate(prefix = "brokenmig", version = 1)]
    pub struct Broken {
        #[amestate(default = 1u64)]
        pub value: u64,
    }
}

#[amethystate(prefix = "brokenmig", version = 2)]
pub struct Broken {
    #[amestate(default = 1u64)]
    pub value: u64,

    #[amestate(default = 0u64)]
    pub added: u64,
}

#[migrate]
fn migrate_broken_v1_to_v2(
    _old: AmeData<v1::Broken>,
    _ctx: &mut amethystate::migration::MigrationContext,
) -> amethystate::MigrationResult<AmeData<Broken>> {
    Err(amethystate::migration::MigrationError::Custom("refused".into()).into())
}

/// The snapshot records what the code declared, so the next run can say what
/// changed. Writing it for a prefix whose migration failed left drift detection
/// comparing the new schema against itself, and the diagnostic for the one
/// prefix that needed it was gone for good.
#[test]
fn a_failed_migration_leaves_the_snapshot_for_the_next_run() {
    let path = unique_path("failed_migration_snapshot");

    {
        let store = StoreBuilder::new(&path).build().unwrap();
        let _ = v1::Broken::new_with(&store);
        store.save_now().unwrap();
    }

    let (_store, report) = StoreBuilder::new(&path).build_with_report().unwrap();

    assert!(
        report.has_failures(),
        "the migration is meant to fail: {report:?}"
    );

    let failed: Vec<&String> = report
        .components
        .iter()
        .filter(|c| {
            matches!(
                c.outcome,
                amethystate::migration::ComponentOutcome::Failed { .. }
            )
        })
        .flat_map(|c| c.prefixes.iter())
        .collect();

    assert!(
        failed.iter().any(|p| p.as_str() == "brokenmig"),
        "the failed prefix is what ensure_snapshots skips, so it has to be \
         named in the report: {failed:?}"
    );
}
