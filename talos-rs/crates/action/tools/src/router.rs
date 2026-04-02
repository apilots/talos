use serde_json::Value;
use talos_error::{TalosError, TalosResult};

use crate::invocation::{ToolCallPayload, ToolInvocation};
use crate::mcp::split_mcp_tool_name;
use crate::spec::ToolCatalog;

#[derive(Debug, Clone, Default)]
pub struct ToolRouter;

impl ToolRouter {
    /// Build an invocation from a raw tool name and arguments.
    ///
    /// MCP tools (names matching `mcp__<server>__<tool>`) are automatically
    /// routed to a `ToolCallPayload::Mcp` invocation; all others become
    /// `ToolCallPayload::Function`.
    pub fn build_function_invocation(
        &self,
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        arguments: Value,
        catalog: &ToolCatalog,
    ) -> TalosResult<ToolInvocation> {
        let call_id = call_id.into();
        let tool_name = tool_name.into();
        let invocation = if let Some((server, tool)) = split_mcp_tool_name(&tool_name) {
            ToolInvocation {
                call_id,
                tool_name,
                payload: ToolCallPayload::Mcp {
                    server,
                    tool,
                    arguments,
                },
            }
        } else {
            ToolInvocation::function(call_id, tool_name, arguments)
        };
        self.normalize(invocation, catalog)
    }

    pub fn normalize(
        &self,
        invocation: ToolInvocation,
        catalog: &ToolCatalog,
    ) -> TalosResult<ToolInvocation> {
        let spec = catalog.find(&invocation.tool_name).ok_or_else(|| {
            TalosError::ToolError(format!("Tool '{}' not found", invocation.tool_name))
        })?;
        if invocation.payload.kind() != spec.payload_kind {
            return Err(TalosError::ToolError(format!(
                "Tool '{}' invoked with incompatible payload",
                invocation.tool_name
            )));
        }
        Ok(invocation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{RegisteredToolSpec, ToolCatalog, ToolParallelism};
    use talos_llm_provider::ToolDefinition;

    fn test_catalog() -> ToolCatalog {
        ToolCatalog::new(vec![RegisteredToolSpec::local_function(
            ToolDefinition::new(
                "echo",
                "Echo input",
                Some(serde_json::json!({"type": "object"})),
            ),
            ToolParallelism::ParallelSafe,
            false,
        )])
    }

    #[test]
    fn test_build_function_invocation_matches_catalog_payload_kind() {
        let router = ToolRouter;
        let invocation = router
            .build_function_invocation(
                "call-1",
                "echo",
                serde_json::json!({ "value": "hello" }),
                &test_catalog(),
            )
            .expect("invocation should build");

        assert_eq!(invocation.tool_name, "echo");
    }

    #[test]
    fn test_normalize_rejects_unknown_tool() {
        let router = ToolRouter;
        let error = router
            .build_function_invocation("call-1", "missing", serde_json::json!({}), &test_catalog())
            .expect_err("missing tool should fail");

        assert!(error.to_string().contains("Tool 'missing' not found"));
    }

    fn mcp_catalog() -> ToolCatalog {
        use crate::spec::{default_governance_for_source, ToolPayloadKind, ToolSource};
        ToolCatalog::new(vec![RegisteredToolSpec::new(
            ToolDefinition::new(
                "mcp__github__create_issue",
                "Create a GitHub issue",
                Some(serde_json::json!({"type": "object"})),
            ),
            ToolSource::Mcp,
            ToolPayloadKind::Mcp,
            ToolParallelism::ParallelSafe,
            false,
            default_governance_for_source(ToolSource::Mcp),
        )])
    }

    #[test]
    fn test_mcp_tool_name_routes_to_mcp_payload() {
        let router = ToolRouter;
        let invocation = router
            .build_function_invocation(
                "call-1",
                "mcp__github__create_issue",
                serde_json::json!({ "title": "Bug" }),
                &mcp_catalog(),
            )
            .expect("MCP invocation should build");

        assert_eq!(invocation.tool_name, "mcp__github__create_issue");
        match invocation.payload {
            crate::invocation::ToolCallPayload::Mcp {
                server,
                tool,
                arguments,
            } => {
                assert_eq!(server, "github");
                assert_eq!(tool, "create_issue");
                assert_eq!(arguments, serde_json::json!({ "title": "Bug" }));
            }
            other => panic!("expected Mcp payload, got {:?}", other),
        }
    }
}
