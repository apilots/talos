use schemars::schema::{InstanceType, Schema, SchemaObject};

pub fn json_value_schema(_: &mut schemars::gen::SchemaGenerator) -> Schema {
    Schema::Object(SchemaObject {
        instance_type: Some(InstanceType::Object.into()),
        ..Default::default()
    })
}
