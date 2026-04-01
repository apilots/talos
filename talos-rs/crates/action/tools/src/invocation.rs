use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::ToolContext;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolInvocation {
    pub call_id: String,
    pub tool_name: String,
    pub payload: ToolCallPayload,
}

impl ToolInvocation {
    pub fn function(
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: Value,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            tool_name: tool_name.into(),
            payload: ToolCallPayload::Function { arguments },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCallPayload {
    Function {
        arguments: Value,
    },
    Custom {
        input: String,
    },
    Resource {
        uri: String,
    },
    Mcp {
        server: String,
        tool: String,
        arguments: Value,
    },
}

impl ToolCallPayload {
    pub fn kind(&self) -> crate::spec::ToolPayloadKind {
        match self {
            Self::Function { .. } => crate::spec::ToolPayloadKind::Function,
            Self::Custom { .. } => crate::spec::ToolPayloadKind::Custom,
            Self::Resource { .. } => crate::spec::ToolPayloadKind::Resource,
            Self::Mcp { .. } => crate::spec::ToolPayloadKind::Mcp,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ToolDispatchContext {
    tool_context: ToolContext,
}

impl ToolDispatchContext {
    pub fn new(tool_context: ToolContext) -> Self {
        Self { tool_context }
    }

    pub fn tool_context(&self) -> &ToolContext {
        &self.tool_context
    }

    pub fn into_tool_context(self) -> ToolContext {
        self.tool_context
    }
}
