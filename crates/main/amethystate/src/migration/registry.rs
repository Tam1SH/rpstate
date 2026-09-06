use crate::MigrationContext;
use crate::migration::fields::FieldDescriptor;
use crate::store::StaticPath;

#[derive(Clone)]
pub struct MigrationStepEntry {
    pub prefix: StaticPath,
    pub target_version: u32,
    pub description: &'static str,
    pub struct_name: &'static str,
    pub fields: &'static [FieldDescriptor],
    pub run: fn(&mut MigrationContext) -> crate::migration::StepResult<()>,
}

inventory::collect!(MigrationStepEntry);
