use schemars::JsonSchema;
use serde::de::{Error as DeError, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use ts_rs::TS;

use crate::errors::AppServerErrorBody;
use crate::requests::ActorCredentials;

pub const JSONRPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq, Eq)]
#[serde(untagged)]
pub enum RequestId {
    String(String),
    Number(i32),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: RequestId,
    pub method: String,
    #[serde(default)]
    pub actor_credentials: Option<ActorCredentials>,
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::json_value_schema")]
    #[ts(type = "unknown")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(
        id: RequestId,
        method: impl Into<String>,
        params: Option<serde_json::Value>,
    ) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            method: method.into(),
            actor_credentials: None,
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: RequestId,
    #[schemars(schema_with = "crate::schema::json_value_schema")]
    #[ts(type = "unknown")]
    pub result: serde_json::Value,
}

impl JsonRpcResponse {
    pub fn new(id: RequestId, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    #[schemars(schema_with = "crate::schema::json_value_schema")]
    #[ts(type = "unknown")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS, PartialEq)]
pub struct JsonRpcErrorResponse {
    pub jsonrpc: String,
    pub id: Option<RequestId>,
    pub error: AppServerErrorBody,
}

impl JsonRpcErrorResponse {
    pub fn new(id: Option<RequestId>, error: AppServerErrorBody) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            error,
        }
    }
}

#[derive(Debug, Clone, Serialize, JsonSchema, TS, PartialEq)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
    Error(JsonRpcErrorResponse),
}

impl<'de> Deserialize<'de> for JsonRpcMessage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MessageVisitor;

        impl<'de> Visitor<'de> for MessageVisitor {
            type Value = JsonRpcMessage;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON-RPC 2.0 message object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut object = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    object.insert(key, value);
                }
                let jsonrpc = object
                    .get("jsonrpc")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| A::Error::custom("missing jsonrpc"))?;
                if jsonrpc != JSONRPC_VERSION {
                    return Err(A::Error::custom("jsonrpc must be 2.0"));
                }

                let value = serde_json::Value::Object(object);
                if value.get("error").is_some() {
                    return serde_json::from_value(value)
                        .map(JsonRpcMessage::Error)
                        .map_err(A::Error::custom);
                }
                if value.get("result").is_some() {
                    return serde_json::from_value(value)
                        .map(JsonRpcMessage::Response)
                        .map_err(A::Error::custom);
                }
                if value.get("id").is_some() {
                    return serde_json::from_value(value)
                        .map(JsonRpcMessage::Request)
                        .map_err(A::Error::custom);
                }
                serde_json::from_value(value)
                    .map(JsonRpcMessage::Notification)
                    .map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_map(MessageVisitor)
    }
}

pub fn parse_message_line(line: &str) -> Result<JsonRpcMessage, AppServerErrorBody> {
    serde_json::from_str::<JsonRpcMessage>(line)
        .map_err(|error| AppServerErrorBody::parse_error(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn request_roundtrips() {
        let request = JsonRpcRequest::new(
            RequestId::Number(1),
            "session.get",
            Some(json!({"session_id": "s1"})),
        );
        let value = serde_json::to_string(&request).unwrap();
        let decoded: JsonRpcMessage = serde_json::from_str(&value).unwrap();
        assert!(matches!(decoded, JsonRpcMessage::Request(_)));
    }

    #[test]
    fn malformed_json_maps_to_parse_error() {
        let error = parse_message_line("{").unwrap_err();
        assert_eq!(error.code, crate::errors::PARSE_ERROR);
    }

    #[test]
    fn notification_roundtrips() {
        let notification = JsonRpcNotification::new("initialized", None);
        let value = serde_json::to_string(&notification).unwrap();
        let decoded: JsonRpcMessage = serde_json::from_str(&value).unwrap();
        assert!(matches!(decoded, JsonRpcMessage::Notification(_)));
    }
}
