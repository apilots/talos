use serde_json::Value;

use crate::ToolDefinition;

use super::{ChatCompletionFunctionDefinition, ChatCompletionTool};

pub fn tools(definitions: Option<Vec<ToolDefinition>>) -> Option<Vec<ChatCompletionTool>> {
    definitions.map(|definitions| {
        definitions
            .into_iter()
            .map(|tool| {
                let parameters = tool.parameters.map(|mut schema| {
                    if let Some(object) = schema.as_object_mut() {
                        if object.get("type").and_then(Value::as_str) == Some("object")
                            && !object.contains_key("properties")
                        {
                            object.insert("properties".to_string(), serde_json::json!({}));
                        }
                    }
                    schema
                });
                ChatCompletionTool {
                    r#type: "function".to_string(),
                    function: ChatCompletionFunctionDefinition {
                        name: tool.name,
                        description: tool.description,
                        parameters,
                    },
                }
            })
            .collect()
    })
}
