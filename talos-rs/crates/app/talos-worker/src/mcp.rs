use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_mcp::{PreparedMcpUpdate, TalosMcpRuntime};
use talos_types::{McpDefinitionRef, McpServerRuntimeDescriptor, McpServerSource};

pub fn prepare(
    runtime: &TalosMcpRuntime,
    config: &AgentKernelConfig,
    plugins: Vec<McpServerRuntimeDescriptor>,
    credentials: &talos_service::McpCredentialStore,
) -> TalosResult<PreparedMcpUpdate> {
    let inputs = plugins
        .into_iter()
        .map(|descriptor| {
            let McpServerSource::Plugin { install_id, .. } = &descriptor.source;
            let owner = McpDefinitionRef::Plugin {
                install_id: install_id.clone(),
            };
            let (resolved, startup_error) = match credentials.resolve(&owner, &descriptor.server) {
                Ok(values) => (values, None),
                Err(error) => (None, Some(error)),
            };
            talos_mcp::McpConnectionInput {
                descriptor,
                credentials: resolved,
                startup_error,
            }
        })
        .collect();
    let prepared = runtime.prepare(inputs)?;
    if config.tools.enable_mcp_tools {
        Ok(prepared)
    } else {
        runtime.prepare(vec![])
    }
}
