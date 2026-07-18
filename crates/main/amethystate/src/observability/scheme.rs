use crate::migration::fields::FieldDescriptor;

pub struct SchemaEntry {
    pub prefix: Option<&'static str>,
    pub struct_name: &'static str,
    pub version: u32,
    pub schema_hash: u32,
    pub fields: &'static [FieldDescriptor],
}

inventory::collect!(SchemaEntry);
