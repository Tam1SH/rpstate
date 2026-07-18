use crate::migration::fields::FieldDescriptor;
use crate::migration::registry::MigrationDependency;
use crate::migration::set::MigrationSet;
use crate::store::StorageResult;
use crate::{MigrationContext, MigrationPlan, StateScope};
use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
pub struct MigrationBuilder {
    prefixes: HashMap<String, PrefixPlan>,
}

/// Migration plan for a single database prefix.
#[derive(Default)]
pub(crate) struct PrefixPlan {
    migrator: MigrationPlan,
    dependencies: BTreeSet<String>,
    pub(crate) fields: &'static [FieldDescriptor],
    pub(crate) schema_hash: u32,
}

pub struct PrefixMigrationBuilder<'a> {
    builder: &'a mut MigrationBuilder,
    prefix: String,
}

impl MigrationBuilder {
    pub fn collect_codegen(&mut self) -> &mut Self {
        use crate::migration::registry::MigrationStepEntry;
        use std::collections::HashSet;
        let mut groups: HashMap<&'static str, Vec<&'static MigrationStepEntry>> = HashMap::new();

        for entry in inventory::iter::<MigrationStepEntry> {
            groups.entry(entry.prefix).or_default().push(entry);
        }

        for (prefix, steps) in groups {
            let mut latest_hash = 0;
            let mut max_v = 0;
            let mut latest_fields: &'static [FieldDescriptor] = &[];
            let mut merged_deps = HashSet::new();

            for step in &steps {
                if step.target_version >= max_v {
                    max_v = step.target_version;
                    latest_hash = step.schema_hash;
                    latest_fields = step.fields;
                }

                for dep in step.dependencies {
                    merged_deps.insert(*dep);
                }

                if step.target_version > 0 {
                    self.for_prefix(prefix)
                        .step(step.target_version, step.description, step.run);
                }
            }

            let plan = self.prefix_plan(prefix);
            plan.schema_hash = latest_hash;
            plan.fields = latest_fields;
            for dep in merged_deps {
                plan.dependencies.insert(dep.to_string());
            }
        }
        self
    }

    pub fn for_node<T: StateScope>(&mut self) -> PrefixMigrationBuilder<'_> {
        self.for_prefix(T::PREFIX)
    }

    pub fn for_prefix(&mut self, prefix: impl Into<String>) -> PrefixMigrationBuilder<'_> {
        PrefixMigrationBuilder {
            builder: self,
            prefix: prefix.into(),
        }
    }

    pub(crate) fn prefix_plan(&mut self, prefix: &str) -> &mut PrefixPlan {
        self.prefixes.entry(prefix.to_string()).or_default()
    }

    pub(crate) fn into_set(self) -> MigrationSet {
        let mut set = MigrationSet::default();
        let mut prefixes = self.prefixes.into_iter().collect::<Vec<_>>();

        prefixes.sort_by(|(a, _), (b, _)| a.cmp(b));

        for (prefix, plan) in prefixes {
            let deps: Vec<&str> = plan.dependencies.iter().map(|s| s.as_str()).collect();
            set = set.add(prefix, plan.migrator, plan.schema_hash, plan.fields, &deps);
        }
        set
    }
}

impl PrefixMigrationBuilder<'_> {
    pub fn depends_on<D: MigrationDependency>(&mut self) -> &mut Self {
        let plan = self.builder.prefix_plan(&self.prefix);
        D::register(&mut plan.dependencies);
        self
    }

    pub fn depends_on_raw(&mut self, dependency: impl Into<String>) -> &mut Self {
        self.builder
            .prefix_plan(&self.prefix)
            .dependencies
            .insert(dependency.into());
        self
    }

    pub fn step<F>(&mut self, target_version: u32, description: &str, run: F) -> &mut Self
    where
        F: Fn(&mut MigrationContext) -> StorageResult<()> + Send + Sync + 'static,
    {
        let plan = self.builder.prefix_plan(&self.prefix);
        let migrator = std::mem::take(&mut plan.migrator);
        plan.migrator = migrator.step(target_version, description, run);
        self
    }
}
