use crate::migration::fields::FieldDescriptor;
use crate::migration::meta::{PrefixMeta, SchemaSnapshot, StoredFieldEntry};
use crate::migration::set::MigrationSet;
use crate::migration::{AppliedStep, ComponentOutcome, ComponentResult, NaggingRecord, SchemaDiff};
use crate::observability::SchemaEntry;
use crate::store::MigrationBackendAdapter;
use crate::store::facts::Facts;
use crate::store::moved::{self, Moved, Verdict};
use crate::store::{StorageError, StorageResult};
use crate::{MigrationContext, MigrationError, MigrationPlan, MigrationReport};
use amethystate_core::path::StorePath;
use error_stack::{Report, ResultExt};
use std::collections::HashMap;

fn group_path(prefix: &str) -> StorageResult<StorePath> {
    StorePath::parse_joined(prefix)
        .change_context(StorageError::Path)
        .attach_migrating(prefix)
}

pub trait StorageProvider {
    fn atomic<F, T>(&self, f: F) -> StorageResult<T>
    where
        F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>;
}

pub struct MigrationEngine<'a, P: StorageProvider> {
    provider: &'a P,
}

impl<'a, P: StorageProvider> MigrationEngine<'a, P> {
    pub fn new(provider: &'a P) -> Self {
        Self { provider }
    }

    /// Records the schema the code declares, so a later run can tell what
    /// changed under it.
    ///
    /// Prefixes whose migration failed are skipped: overwriting their snapshot
    /// with the current schema leaves `calculate_drift` nothing to compare
    /// against, and the diagnostic for the one prefix that needs it is gone for
    /// good.
    pub fn ensure_snapshots(&self, failed: &[String]) -> StorageResult<()> {
        self.provider.atomic(|storage| {
            for entry in inventory::iter::<SchemaEntry> {
                let prefix = match &entry.prefix {
                    Some(p) => p,
                    None => continue,
                };

                if failed.iter().any(|p| p == prefix.as_str()) {
                    continue;
                }

                let stored = storage.get_schema_snapshot(prefix).attach_with(|| {
                    format!(
                        "recording the schema of {} v{} at {prefix}",
                        entry.struct_name, entry.version
                    )
                })?;

                let needs_update = match &stored {
                    None => true,
                    Some(s) => {
                        s.struct_name.as_deref() != Some(entry.struct_name)
                            || s.version != entry.version
                            || !moved::between(&s.fields, entry.fields).is_empty()
                    }
                };

                if needs_update {
                    storage
                        .set_schema_snapshot(
                            prefix,
                            &SchemaSnapshot {
                                version: entry.version,
                                struct_name: Some(entry.struct_name.to_string()),
                                fields: entry.fields.iter().map(StoredFieldEntry::from).collect(),
                            },
                        )
                        .attach_with(|| {
                            format!(
                                "recording the schema of {} v{} at {prefix}",
                                entry.struct_name, entry.version
                            )
                        })?;
                }
            }
            Ok(())
        })
    }

    pub fn run(&self, mset: MigrationSet) -> StorageResult<MigrationReport> {
        let mut report = MigrationReport::default();
        let components = mset.find_components();

        for component_prefixes in components {
            let sorted_prefixes = mset.topo_sort_component(&component_prefixes)?;

            let outcome_res = self.provider.atomic(|storage| {
                if !self.component_needs_work(storage, &sorted_prefixes, &mset)? {
                    return Ok((ComponentOutcome::Skipped, Vec::new()));
                }

                match self.execute_component_migration(storage, &sorted_prefixes, &mset) {
                    Ok((steps, nagging)) => Ok((ComponentOutcome::Committed { steps }, nagging)),
                    Err(e) => Err(e),
                }
            });

            match outcome_res {
                Ok((outcome, nagging)) => {
                    report.components.push(ComponentResult {
                        prefixes: component_prefixes,
                        outcome,
                        nagging,
                    });
                }
                Err(e) => {
                    report.components.push(ComponentResult {
                        prefixes: component_prefixes,
                        outcome: ComponentOutcome::Failed { error: e },
                        nagging: Vec::new(),
                    });
                }
            }
        }

        let failed: Vec<String> = report
            .components
            .iter()
            .filter(|c| matches!(c.outcome, ComponentOutcome::Failed { .. }))
            .flat_map(|c| c.prefixes.iter().cloned())
            .collect();

        self.ensure_snapshots(&failed)?;

        Ok(report)
    }

    fn component_needs_work(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefixes: &[String],
        mset: &MigrationSet,
    ) -> StorageResult<bool> {
        for prefix in prefixes {
            let meta = storage.get_meta(&group_path(prefix)?)?;
            let current_v = meta.as_ref().map(|m| m.version).unwrap_or(0);
            let (target_v, target_fields) = mset.get_target(prefix);

            if target_v != current_v {
                return Ok(true);
            }

            if !self
                .places_that_moved(storage, prefix, target_fields)?
                .is_empty()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn execute_component_migration(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefixes: &[String],
        mset: &MigrationSet,
    ) -> StorageResult<(Vec<AppliedStep>, Vec<NaggingRecord>)> {
        let mut all_steps = Vec::new();
        let mut all_nagging = Vec::new();

        for prefix in prefixes {
            let (steps, nagging) = self.migrate_prefix(storage, prefix, mset)?;
            all_steps.extend(steps);
            all_nagging.extend(nagging);
        }

        Ok((all_steps, all_nagging))
    }

    /// Where the declared places sit now against where they sat when this
    /// prefix was last written.
    ///
    /// Empty when nothing was written before: a store being opened for the
    /// first time has nothing to have moved from.
    fn places_that_moved(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefix: &str,
        current_fields: &[FieldDescriptor],
    ) -> StorageResult<Vec<Moved>> {
        let Some(old) = storage.get_schema_snapshot(&group_path(prefix)?)? else {
            return Ok(Vec::new());
        };

        Ok(moved::between(&old.fields, current_fields))
    }

    fn calculate_drift(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefix: &str,
        current_fields: &[FieldDescriptor],
    ) -> StorageResult<Option<SchemaDiff>> {
        let snapshot = storage.get_schema_snapshot(&group_path(prefix)?)?;
        let Some(old) = snapshot else {
            return Ok(None);
        };

        let mut diff = SchemaDiff {
            added: vec![],
            removed: vec![],
        };

        let mut old_fields: HashMap<StorePath, StoredFieldEntry> = old
            .fields
            .into_iter()
            .map(|f| (f.name.clone(), f))
            .collect();

        for f in current_fields {
            if old_fields.remove(f.name.as_str()).is_none() {
                diff.added.push(StoredFieldEntry::from(f));
            }
        }

        diff.removed = old_fields.into_values().collect();

        if diff.added.is_empty() && diff.removed.is_empty() {
            Ok(None)
        } else {
            Ok(Some(diff))
        }
    }

    fn migrate_prefix(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefix: &str,
        mset: &MigrationSet,
    ) -> StorageResult<(Vec<AppliedStep>, Vec<NaggingRecord>)> {
        let (target_v, target_fields) = mset.get_target(prefix);
        let prefix_path = group_path(prefix)?;

        let meta_opt = storage.get_meta(&prefix_path)?;

        let mut meta = match meta_opt {
            Some(m) => m,
            None => {
                let start_v = mset
                    .get_migration_plan(prefix)
                    .and_then(|m| m.steps.iter().map(|s| s.target_version()).min())
                    .map(|v| v.saturating_sub(1))
                    .unwrap_or(target_v);

                if start_v == target_v {
                    storage.set_meta(&prefix_path, &PrefixMeta { version: target_v })?;
                    return Ok((vec![], vec![]));
                }

                PrefixMeta { version: start_v }
            }
        };

        let mut nagging = Vec::new();

        if target_v < meta.version {
            return Err(Report::new(MigrationError::Downgrade {
                prefix: prefix.to_string(),
                db_version: meta.version,
                code_version: target_v,
            })
            .change_context(StorageError::Migrate));
        }

        if target_v == meta.version {
            let moved = self.places_that_moved(storage, prefix, target_fields)?;

            if moved.iter().any(|one| one.verdict() == Verdict::Breaks) {
                let diff = self.calculate_drift(storage, prefix, target_fields)?;

                nagging.push(NaggingRecord {
                    prefix: prefix.to_string(),
                    diff,
                    moved,
                });
            }
        }

        let mut applied_steps = Vec::new();
        if let Some(plan) = mset.get_migration_plan(prefix) {
            let mut history = storage.get_migration_log(&prefix_path)?.unwrap_or_default();

            applied_steps = self.run_migrator_steps(
                storage,
                prefix,
                plan,
                &mut meta,
                target_v,
                &mut history,
                mset.provided(),
            )?;

            if !applied_steps.is_empty() {
                storage.set_meta(&prefix_path, &meta)?;
                storage.set_migration_log(&prefix_path, &history)?;
            }
        }

        let unanswered = !nagging.is_empty() && applied_steps.is_empty();

        if meta.version == target_v && !target_fields.is_empty() && !unanswered {
            let struct_name = inventory::iter::<SchemaEntry>
                .into_iter()
                .find(|e| e.prefix.as_ref() == Some(&prefix_path))
                .map(|e| e.struct_name.to_string());

            let new_snapshot = SchemaSnapshot {
                version: target_v,
                struct_name,
                fields: target_fields.iter().map(StoredFieldEntry::from).collect(),
            };

            storage.set_schema_snapshot(&prefix_path, &new_snapshot)?;
        }

        Ok((applied_steps, nagging))
    }

    fn run_migrator_steps(
        &self,
        storage: &mut dyn MigrationBackendAdapter,
        prefix: &str,
        migrator: &MigrationPlan,
        meta: &mut PrefixMeta,
        target_v: u32,
        history: &mut Vec<AppliedStep>,
        provided: &crate::migration::provided::Provided,
    ) -> StorageResult<Vec<AppliedStep>> {
        let mut new_steps = Vec::new();
        let mut ctx = MigrationContext::new(prefix.to_string(), storage).with_provided(provided);

        for step in &migrator.steps {
            let sv = step.target_version();
            if sv <= meta.version {
                continue;
            }
            if sv > target_v {
                break;
            }

            if sv != meta.version + 1 {
                return Err(Report::new(MigrationError::Gap {
                    prefix: prefix.to_string(),
                    reached_version: meta.version,
                    expected_version: meta.version + 1,
                })
                .change_context(StorageError::Migrate));
            }

            step.run(&mut ctx)?;

            let applied = AppliedStep {
                prefix: prefix.to_string(),
                target_version: sv,
                description: step.description().map(|s| s.to_string()),
                applied_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            };

            meta.version = sv;
            history.push(applied.clone());
            new_steps.push(applied);
        }

        Ok(new_steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amethystate_core::path::StorePath;

    /// The mock adapter is the trait, so it wants a built path rather than the
    /// levels a store call would take.
    fn p(name: &str) -> StorePath {
        StorePath::from_segments([name])
    }

    use crate::migration::context::{decode, encode};
    use crate::migration::fields::FieldDescriptor;
    use crate::migration::meta::StoredShape;
    use crate::store::{CodecFormat, IntoStorageReport, StorageError};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::ops::Deref;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tracing_test::traced_test;

    const EMPTY_FIELDS: &[FieldDescriptor] = &[];

    #[derive(Default, Clone)]
    struct InMemoryStorage {
        data: HashMap<String, Vec<u8>>,
        meta: HashMap<String, PrefixMeta>,
        snapshots: HashMap<String, SchemaSnapshot>,
        logs: HashMap<String, Vec<AppliedStep>>,
    }

    impl InMemoryStorage {
        fn get_decoded<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
            self.data.get(key).map(|b| decode(self, b).unwrap())
        }
    }

    impl MigrationBackendAdapter for InMemoryStorage {
        fn format(&self) -> CodecFormat {
            CodecFormat::Default
        }

        fn get(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
            Ok(self.data.get(key).cloned())
        }
        fn set(&mut self, key: &str, value: &[u8]) -> StorageResult<()> {
            self.data.insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn delete(&mut self, key: &str) -> StorageResult<()> {
            self.data.remove(key);
            Ok(())
        }
        fn scan_prefix(&self, prefix: &StorePath) -> StorageResult<Vec<(StorePath, Vec<u8>)>> {
            let mut res = Vec::new();
            for (k, v) in &self.data {
                if k.starts_with(prefix.as_str()) {
                    res.push((StorePath::parse_joined(k).unwrap(), v.clone()));
                }
            }
            Ok(res)
        }
        fn get_meta(&self, prefix: &StorePath) -> StorageResult<Option<PrefixMeta>> {
            Ok(self.meta.get(prefix.as_str()).cloned())
        }
        fn set_meta(&mut self, prefix: &StorePath, meta: &PrefixMeta) -> StorageResult<()> {
            self.meta.insert(prefix.to_string(), meta.clone());
            Ok(())
        }
        fn get_schema_snapshot(&self, prefix: &StorePath) -> StorageResult<Option<SchemaSnapshot>> {
            Ok(self.snapshots.get(prefix.as_str()).cloned())
        }
        fn set_schema_snapshot(
            &mut self,
            prefix: &StorePath,
            snapshot: &SchemaSnapshot,
        ) -> StorageResult<()> {
            self.snapshots.insert(prefix.to_string(), snapshot.clone());
            Ok(())
        }
        fn get_migration_log(&self, prefix: &StorePath) -> StorageResult<Option<Vec<AppliedStep>>> {
            Ok(self.logs.get(prefix.as_str()).cloned())
        }
        fn set_migration_log(
            &mut self,
            prefix: &StorePath,
            log: &[AppliedStep],
        ) -> StorageResult<()> {
            self.logs.insert(prefix.to_string(), log.to_vec());
            Ok(())
        }
    }

    impl StorageProvider for RefCell<InMemoryStorage> {
        fn atomic<F, T>(&self, f: F) -> StorageResult<T>
        where
            F: FnOnce(&mut dyn MigrationBackendAdapter) -> StorageResult<T>,
        {
            let backup = self.borrow().clone();

            let res = {
                let mut guard = self.borrow_mut();
                f(&mut *guard)
            };

            match res {
                Ok(t) => Ok(t),
                Err(e) => {
                    *self.borrow_mut() = backup;
                    Err(e)
                }
            }
        }
    }

    #[test]
    fn test_first_initialization() {
        let storage = RefCell::new(InMemoryStorage::default());
        let mset = MigrationSet::default().add(
            "ui",
            MigrationPlan::new().step(1, "init", |_| Ok(())),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(!report.has_failures());
        let meta = storage.borrow().get_meta(&p("ui")).unwrap().unwrap();
        assert_eq!(meta.version, 1);
    }

    #[test]
    fn test_missing_migration_step_does_not_advance_meta() {
        let storage = RefCell::new(InMemoryStorage::default());
        storage
            .borrow_mut()
            .set_meta(&p("app"), &PrefixMeta { version: 1 })
            .unwrap();

        let mset = MigrationSet::default().add(
            "app",
            MigrationPlan::new().step(3, "v3", |_| Ok(())),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        let ComponentOutcome::Failed { error } = &report.components[0].outcome else {
            panic!("Expected failed migration component");
        };

        assert_eq!(error.current_context(), &StorageError::Migrate);
        let Some(MigrationError::Gap {
            prefix,
            reached_version,
            expected_version,
        }) = error.downcast_ref::<MigrationError>()
        else {
            panic!("Expected migration gap, got {error:?}");
        };

        assert_eq!(prefix, "app");
        assert_eq!(*reached_version, 1);
        assert_eq!(*expected_version, 2);

        let meta = storage.borrow().get_meta(&p("app")).unwrap().unwrap();
        assert_eq!(meta.version, 1);
    }

    #[test]
    fn test_downgrade_error() {
        let storage = RefCell::new(InMemoryStorage::default());
        storage
            .borrow_mut()
            .set_meta(&p("app"), &PrefixMeta { version: 5 })
            .unwrap();

        let mset = MigrationSet::default().add(
            "app",
            MigrationPlan::new().step(4, "v4", |_| Ok(())),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        let ComponentOutcome::Failed { error } = &report.components[0].outcome else {
            panic!("Expected failed migration component");
        };

        if let Some(MigrationError::Downgrade {
            db_version,
            code_version,
            ..
        }) = error.downcast_ref::<MigrationError>()
        {
            assert_eq!(*db_version, 5);
            assert_eq!(*code_version, 4);
        } else {
            panic!("Expected Downgrade error, got {error:?}");
        }
    }

    #[test]
    fn test_independent_components_success() {
        let storage = RefCell::new(InMemoryStorage::default());
        let mset = MigrationSet::default()
            .add(
                "a",
                MigrationPlan::new().step(1, "ok", |ctx| ctx.set("v", &1)),
                EMPTY_FIELDS,
                &[],
            )
            .add(
                "b",
                MigrationPlan::new().step(1, "fail", |_| {
                    Err(MigrationError::Custom("err".into()).into_report())
                }),
                EMPTY_FIELDS,
                &[],
            );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(report.has_failures());
        assert_eq!(storage.borrow().get_decoded::<i32>("a.v").unwrap(), 1);
    }

    #[test]
    fn test_idle_migration_skipped() {
        let storage = RefCell::new(InMemoryStorage::default());
        storage
            .borrow_mut()
            .set_meta(&p("app"), &PrefixMeta { version: 1 })
            .unwrap();
        let val = encode(storage.borrow().deref(), &1).unwrap();

        storage.borrow_mut().data.insert("app.v".into(), val);

        let mset = MigrationSet::default().add(
            "app",
            MigrationPlan::new().step(1, "init", |_| Ok(())),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(matches!(
            report.components[0].outcome,
            ComponentOutcome::Skipped
        ));
    }

    #[test]
    fn test_partial_migration_within_component() {
        let storage = RefCell::new(InMemoryStorage::default());
        storage
            .borrow_mut()
            .set_meta(&p("a"), &PrefixMeta { version: 1 })
            .unwrap();

        let a_calls = Arc::new(AtomicUsize::new(0));
        let b_calls = Arc::new(AtomicUsize::new(0));

        let a_cap = a_calls.clone();
        let b_cap = b_calls.clone();

        let mset = MigrationSet::default()
            .add(
                "a",
                MigrationPlan::new().step(1, "v1", move |_| {
                    a_cap.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
                EMPTY_FIELDS,
                &[],
            )
            .add(
                "b",
                MigrationPlan::new().step(1, "v1", move |_| {
                    b_cap.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }),
                EMPTY_FIELDS,
                &["a"],
            );

        let engine = MigrationEngine::new(&storage);
        engine.run(mset).unwrap();

        assert_eq!(a_calls.load(Ordering::SeqCst), 0);
        assert_eq!(b_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_multiple_steps_migration_order() {
        let storage = RefCell::new(InMemoryStorage::default());
        let mset = MigrationSet::default().add(
            "app",
            MigrationPlan::new()
                .step(1, "one", |ctx| ctx.set("log", &"1".to_string()))
                .step(2, "two", |ctx| {
                    let mut s: String = ctx.get("log")?.unwrap();
                    s.push('2');
                    ctx.set("log", &s)
                })
                .step(3, "three", |ctx| {
                    let mut s: String = ctx.get("log")?.unwrap();
                    s.push('3');
                    ctx.set("log", &s)
                }),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        engine.run(mset).unwrap();

        let final_log: String = storage.borrow().get_decoded("app.log").unwrap();
        assert_eq!(final_log, "123");
    }

    #[test]
    fn test_migration_resume_from_version() {
        let storage = RefCell::new(InMemoryStorage::default());
        storage
            .borrow_mut()
            .set_meta(&p("app"), &PrefixMeta { version: 1 })
            .unwrap();

        let val = encode(storage.borrow().deref(), &"1").unwrap();
        storage.borrow_mut().data.insert("app.log".into(), val);

        let mset = MigrationSet::default().add(
            "app",
            MigrationPlan::new()
                .step(1, "init", |_| panic!("Step 1 should be skipped"))
                .step(2, "next", |ctx| {
                    let mut s: String = ctx.get("log")?.unwrap();
                    s.push('2');
                    ctx.set("log", &s)
                }),
            EMPTY_FIELDS,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        engine.run(mset).unwrap();

        let final_log: String = storage.borrow().get_decoded("app.log").unwrap();
        assert_eq!(final_log, "12");
    }

    #[test]
    fn a_field_added_beside_the_others_is_not_drift() {
        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("profile");

        storage
            .borrow_mut()
            .set_meta(prefix, &PrefixMeta { version: 1 })
            .unwrap();
        storage
            .borrow_mut()
            .set_schema_snapshot(
                prefix,
                &SchemaSnapshot {
                    version: 1,
                    fields: vec![StoredFieldEntry {
                        name: StorePath::segment("name"),
                        type_name: "String".to_string(),
                        shape: StoredShape::field(),
                    }],
                    struct_name: None,
                },
            )
            .unwrap();

        static CURRENT_FIELDS: &[FieldDescriptor] = &[
            FieldDescriptor::leaf(&["name"], "name", "String"),
            FieldDescriptor::leaf(&["age"], "age", "u32"),
        ];
        let current_fields = CURRENT_FIELDS;

        let mset = MigrationSet::default().add(
            prefix.as_str(),
            MigrationPlan::new().step(1, "v1", |_| Ok(())),
            current_fields,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(
            !report.has_drift(),
            "`age` takes a place nothing declared before, and nothing that was \
             written has moved out from under anything"
        );
    }

    #[test]
    fn a_field_no_longer_declared_is_drift() {
        use crate::store::moved::What;

        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("profile");

        storage
            .borrow_mut()
            .set_meta(prefix, &PrefixMeta { version: 1 })
            .unwrap();
        storage
            .borrow_mut()
            .set_schema_snapshot(
                prefix,
                &SchemaSnapshot {
                    version: 1,
                    fields: vec![
                        StoredFieldEntry {
                            name: StorePath::segment("name"),
                            type_name: "String".to_string(),
                            shape: StoredShape::field(),
                        },
                        StoredFieldEntry {
                            name: StorePath::segment("nickname"),
                            type_name: "String".to_string(),
                            shape: StoredShape::field(),
                        },
                    ],
                    struct_name: None,
                },
            )
            .unwrap();

        static CURRENT_FIELDS: &[FieldDescriptor] =
            &[FieldDescriptor::leaf(&["name"], "name", "String")];

        let mset = MigrationSet::default().add(
            prefix.as_str(),
            MigrationPlan::new().step(1, "v1", |_| Ok(())),
            CURRENT_FIELDS,
            &[],
        );

        let report = MigrationEngine::new(&storage).run(mset).unwrap();

        assert!(report.has_drift());

        let moved = &report.components[0].nagging[0].moved;
        let released: Vec<&str> = moved
            .iter()
            .filter(|one| one.what == What::Released)
            .map(|one| one.at.as_str())
            .collect();

        assert_eq!(released, ["nickname"], "and it names the place: {moved:?}");
        assert!(moved.iter().any(|one| one.verdict() == Verdict::Breaks));
    }

    #[test]
    fn a_type_that_changed_under_one_name_is_the_readers_business() {
        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("settings");

        storage
            .borrow_mut()
            .set_meta(prefix, &PrefixMeta { version: 1 })
            .unwrap();
        storage
            .borrow_mut()
            .set_schema_snapshot(
                prefix,
                &SchemaSnapshot {
                    version: 1,
                    fields: vec![StoredFieldEntry {
                        name: StorePath::segment("port"),
                        type_name: "u16".to_string(),
                        shape: StoredShape::field(),
                    }],
                    struct_name: None,
                },
            )
            .unwrap();

        static CURRENT_FIELDS: &[FieldDescriptor] =
            &[FieldDescriptor::leaf(&["port"], "port", "u32")];
        let current_fields = CURRENT_FIELDS;

        let mset = MigrationSet::default().add(
            prefix.as_str(),
            MigrationPlan::new().step(1, "v1", |_| Ok(())),
            current_fields,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(
            !report.has_drift(),
            "`port` is where it was; that it holds a different type is answered \
             where it is read, not here"
        );
    }

    #[test]
    fn test_drift_nagging_persists_until_migration() {
        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("app");

        storage
            .borrow_mut()
            .set_meta(prefix, &PrefixMeta { version: 1 })
            .unwrap();
        storage
            .borrow_mut()
            .set_schema_snapshot(
                prefix,
                &SchemaSnapshot {
                    version: 1,
                    fields: vec![StoredFieldEntry {
                        name: StorePath::segment("old"),
                        type_name: "i32".to_string(),
                        shape: StoredShape::field(),
                    }],
                    struct_name: None,
                },
            )
            .unwrap();

        static NEW_FIELDS: &[FieldDescriptor] = &[FieldDescriptor::leaf(&["new"], "new", "i32")];
        let fields = NEW_FIELDS;

        {
            let mset = MigrationSet::default().add(
                prefix.as_str(),
                MigrationPlan::new().step(1, "v1", |_| Ok(())),
                fields,
                &[],
            );
            let engine = MigrationEngine::new(&storage);
            let report = engine.run(mset).unwrap();
            assert!(report.has_drift(), "Should nag on the first run");
        }

        {
            let mset = MigrationSet::default().add(
                prefix.as_str(),
                MigrationPlan::new().step(1, "v1", |_| Ok(())),
                fields,
                &[],
            );
            let engine = MigrationEngine::new(&storage);
            let report = engine.run(mset).unwrap();
            assert!(
                report.has_drift(),
                "Should STILL nag! The dev hasn't bumped the version!"
            );
        }

        {
            let mset = MigrationSet::default().add(
                prefix.as_str(),
                MigrationPlan::new()
                    .step(1, "v1", |_| Ok(()))
                    .step(2, "ack_drift", |_| Ok(())),
                fields,
                &[],
            );
            let engine = MigrationEngine::new(&storage);
            let report = engine.run(mset).unwrap();

            assert!(!report.has_failures());
            assert!(
                !report.has_drift(),
                "Drift resolved because version was bumped!"
            );
        }

        let meta = storage.borrow().get_meta(prefix).unwrap().unwrap();
        assert_eq!(meta.version, 2);
    }

    #[test]
    fn test_migration_updates_snapshot() {
        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("data");

        storage
            .borrow_mut()
            .set_meta(prefix, &PrefixMeta { version: 1 })
            .unwrap();
        storage
            .borrow_mut()
            .set_schema_snapshot(
                prefix,
                &SchemaSnapshot {
                    version: 1,
                    fields: vec![StoredFieldEntry {
                        name: StorePath::segment("old_f"),
                        type_name: "u8".into(),
                        shape: StoredShape::field(),
                    }],
                    struct_name: None,
                },
            )
            .unwrap();

        static V2_FIELDS: &[FieldDescriptor] = &[FieldDescriptor::leaf(&["new_f"], "new_f", "u16")];
        let v2_fields = V2_FIELDS;

        let mset = MigrationSet::default().add(
            prefix.as_str(),
            MigrationPlan::new().step(2, "v2", |ctx| ctx.set("new_f", &10u16)),
            v2_fields,
            &[],
        );

        let engine = MigrationEngine::new(&storage);
        let report = engine.run(mset).unwrap();

        assert!(
            report.components[0].nagging.is_empty(),
            "Nagging must remain empty during active upgrades"
        );

        let snap = storage
            .borrow()
            .get_schema_snapshot(prefix)
            .unwrap()
            .unwrap();
        assert_eq!(snap.version, 2);
        assert_eq!(snap.fields.len(), 1);
        assert_eq!(snap.fields[0].name.as_str(), "new_f");
        assert_eq!(snap.fields[0].type_name, "u16");
    }

    #[traced_test]
    #[test]
    fn test_drift_automatic_warning_log() {
        let storage = RefCell::new(InMemoryStorage::default());
        let prefix = &p("app_settings");

        {
            static FIELDS_V1: &[FieldDescriptor] = &[
                FieldDescriptor::leaf(&["port"], "port", "u16"),
                FieldDescriptor::leaf(&["host"], "host", "String"),
            ];
            let fields_v1 = FIELDS_V1;

            let mset = MigrationSet::default().add(
                prefix.as_str(),
                MigrationPlan::new().step(1, "v1", |_| Ok(())),
                fields_v1,
                &[],
            );

            let engine = MigrationEngine::new(&storage);
            let _ = engine.run(mset).unwrap();
        }

        {
            static FIELDS_V2: &[FieldDescriptor] = &[
                FieldDescriptor::leaf(&["port"], "port", "u32"),
                FieldDescriptor::leaf(&["timeout"], "timeout", "Duration"),
            ];
            let fields_v2 = FIELDS_V2;

            let mset = MigrationSet::default().add(
                prefix.as_str(),
                MigrationPlan::new().step(1, "v1", |_| Ok(())),
                fields_v2,
                &[],
            );

            let engine = MigrationEngine::new(&storage);
            let report = engine.run(mset).unwrap();

            assert!(report.has_drift(), "Report should detect drift");
        }
    }
}
