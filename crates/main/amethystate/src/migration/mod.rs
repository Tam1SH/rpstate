use tracing::{info, warn};

pub mod builder;
pub mod context;
pub mod engine;
pub mod error;
pub mod fields;
pub mod migrate_from;
pub mod node;
pub mod registry;
pub mod set;
pub mod types;

use crate::store::{StorageError, StorageResult, meta};
pub use context::MigrationContext;
pub use error::MigrationError;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchemaDiff {
    pub added: Vec<meta::StoredFieldEntry>,
    pub removed: Vec<meta::StoredFieldEntry>,
    pub type_changed: Vec<FieldTypeChange>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldTypeChange {
    pub name: String,
    pub old_type: String,
    pub new_type: String,
}

#[derive(Debug, Clone)]
pub struct NaggingRecord {
    pub prefix: String,
    pub old_hash: u32,
    pub new_hash: u32,
    pub diff: Option<SchemaDiff>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AppliedStep {
    pub prefix: String,
    pub target_version: u32,
    pub description: Option<String>,
    pub applied_at: u64,
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub components: Vec<ComponentResult>,
}

#[derive(Debug)]
pub struct ComponentResult {
    pub prefixes: Vec<String>,
    pub outcome: ComponentOutcome,
    pub nagging: Vec<NaggingRecord>,
}

#[derive(Debug)]
pub enum ComponentOutcome {
    Committed { steps: Vec<AppliedStep> },
    Skipped,
    Failed { error: StorageError },
}

impl MigrationReport {
    pub fn has_failures(&self) -> bool {
        self.components
            .iter()
            .any(|c| matches!(c.outcome, ComponentOutcome::Failed { .. }))
    }
    pub fn has_drift(&self) -> bool {
        self.components.iter().any(|c| !c.nagging.is_empty())
    }

    pub fn log_to_tracing(&self) {
        for comp in &self.components {
            for nag in &comp.nagging {
                warn!("⚠️  Schema drift detected in prefix '{}'", nag.prefix);
                if let Some(diff) = &nag.diff {
                    for f in &diff.added {
                        warn!("  + field '{}': {}", f.name, f.type_name);
                    }
                    for f in &diff.removed {
                        warn!("  - field '{}' (exists in DB, missing in code)", f.name);
                    }
                    for c in &diff.type_changed {
                        warn!("  ~ field '{}': {} -> {}", c.name, c.old_type, c.new_type);
                    }
                }
                warn!(
                    "  Suggestion: increment version and write a migration if these changes are intentional."
                );
            }

            match &comp.outcome {
                ComponentOutcome::Committed { steps } => {
                    for step in steps {
                        info!(
                            "✅ Applied: {} v{} ({})",
                            step.prefix,
                            step.target_version,
                            step.description.as_deref().unwrap_or("no description")
                        );
                    }
                }
                ComponentOutcome::Failed { error } => {
                    tracing::error!("❌ Component {:?} failed: {}", comp.prefixes, error);
                    tracing::error!(
                        "   Transaction rolled back. Data for these prefixes remains unchanged."
                    );
                }
                ComponentOutcome::Skipped => {
                    tracing::debug!("⏩ Component {:?} is up to date", comp.prefixes);
                }
            }
        }
    }
}

pub trait Migration: Send + Sync {
    fn target_version(&self) -> u32;
    fn description(&self) -> Option<&str> {
        None
    }
    fn run(&self, ctx: &mut MigrationContext) -> StorageResult<()>;
}

pub struct MigrationPlan {
    pub(crate) steps: Vec<Box<dyn Migration>>,
}

impl MigrationPlan {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    pub fn step<F>(mut self, version: u32, description: &str, f: F) -> Self
    where
        F: Fn(&mut MigrationContext) -> StorageResult<()> + Send + Sync + 'static,
    {
        struct ClosureMigration<F> {
            v: u32,
            d: String,
            f: F,
        }
        impl<F> Migration for ClosureMigration<F>
        where
            F: Fn(&mut MigrationContext) -> StorageResult<()> + Send + Sync + 'static,
        {
            fn target_version(&self) -> u32 {
                self.v
            }
            fn description(&self) -> Option<&str> {
                Some(&self.d)
            }
            fn run(&self, ctx: &mut MigrationContext) -> StorageResult<()> {
                (self.f)(ctx)
            }
        }

        self.steps.push(Box::new(ClosureMigration {
            v: version,
            d: description.to_string(),
            f,
        }));
        self.steps.sort_by_key(|s| s.target_version());
        self
    }
}

impl Default for MigrationPlan {
    fn default() -> Self {
        Self::new()
    }
}
