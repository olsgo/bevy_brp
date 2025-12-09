//! `registry.schema` tool - Get type schemas

use bevy_brp_mcp_macros::ParamStruct;
use bevy_brp_mcp_macros::ResultStruct;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::brp_tools::Port;

/// Parameters for the `registry.schema` tool
#[derive(Clone, Deserialize, Serialize, JsonSchema, ParamStruct)]
pub struct RegistrySchemaParams {
    /// Include only types from these crates (e.g., [`bevy_transform`, `my_game`])
    #[serde(default)]
    pub with_crates: Vec<String>,

    /// Include only types with these reflect traits (e.g., [`Component`, `Resource`])
    #[serde(default)]
    pub with_types: Vec<String>,

    /// Exclude types from these crates (e.g., [`bevy_render`, `bevy_pbr`])
    #[serde(default)]
    pub without_crates: Vec<String>,

    /// Exclude types with these reflect traits (e.g., [`RenderResource`])
    #[serde(default)]
    pub without_types: Vec<String>,

    /// The BRP port (default: 15702)
    #[serde(default)]
    pub port: Port,
}

/// Result for the `registry.schema` tool
#[derive(Serialize, ResultStruct)]
#[brp_result]
pub struct RegistrySchemaResult {
    /// The raw BRP response - array of type schemas
    #[serde(skip_serializing_if = "Option::is_none")]
    #[to_result(skip_if_none)]
    pub result: Option<Value>,

    /// Count of types returned
    #[to_metadata(result_operation = "count")]
    pub type_count: usize,

    /// Message template for formatting responses
    #[to_message(message_template = "Retrieved {type_count} schemas")]
    pub message_template: String,
}

#[cfg(test)]
mod tests {
    use super::RegistrySchemaParams;
    use schemars::schema_for;

    #[test]
    fn registry_schema_params_arrays_are_non_nullable() {
        let schema = schema_for!(RegistrySchemaParams);
        let value = serde_json::to_value(&schema).expect("serialize schema");

        println!("{}", serde_json::to_string_pretty(&value).unwrap());

        let props = value
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("properties object");

        for field in [
            "with_crates",
            "with_types",
            "without_crates",
            "without_types",
        ] {
            let prop = props.get(field).expect("property exists");

            let ty = prop
                .get("type")
                .and_then(|v| v.as_str())
                .expect("array type string");
            assert_eq!(ty, "array", "{field} should be an array type");

            let item_ty = prop
                .get("items")
                .and_then(|items| items.get("type"))
                .and_then(|v| v.as_str())
                .expect("items.type string");
            assert_eq!(item_ty, "string", "{field} items should be strings");
        }
    }
}
