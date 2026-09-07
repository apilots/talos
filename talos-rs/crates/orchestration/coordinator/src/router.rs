use std::sync::Arc;

use talos_error::TalosResult;
use talos_router::Router;
use talos_types::talos::SessionSource;

use crate::AgentRegistry;

/// Parameters used by Coordinator to select an Agent.
///
/// This structure is an internal orchestration input, not a Gateway or SDK
/// request DTO. External product APIs should not expose these fields until the
/// multi-Agent product selection model is explicitly designed.
#[derive(Debug, Clone)]
pub struct AgentSelectionParams {
    /// Trusted Session source used by routing policy.
    pub source: SessionSource,
    /// Authenticated User identity used by routing policy.
    pub user_id: String,
}

impl AgentSelectionParams {
    /// Build selection params from a trusted Session source and authenticated User.
    pub fn for_session_source(source: SessionSource, user_id: String) -> Self {
        Self { source, user_id }
    }
}

/// Coordinator-owned Agent selection framework.
///
/// AgentRouter resolves one product routing-policy binding and validates the
/// selected Agent against the live registry. It must not invent fallback
/// selection, parse Provider-private config, or use workspace_id directly.
#[derive(Clone)]
pub(crate) struct AgentRouter {
    routing_policy: Arc<dyn Router>,
    registry: AgentRegistry,
}

impl AgentRouter {
    /// Build a Coordinator Agent router.
    pub(crate) fn new(routing_policy: Arc<dyn Router>, registry: AgentRegistry) -> Self {
        Self {
            routing_policy,
            registry,
        }
    }

    /// Resolve and validate the Agent selected by product routing policy.
    ///
    /// Composition Root installs the configured default as a routing binding;
    /// Coordinator therefore rejects a missing route instead of inventing one.
    pub(crate) async fn resolve_agent(&self, params: AgentSelectionParams) -> TalosResult<String> {
        let route_channel = route_channel(&params.source);
        let agent_id = self
            .routing_policy
            .route(&route_channel, &params.user_id)
            .await?
            .and_then(|agent_id| normalized(Some(agent_id.as_str())).map(str::to_string))
            .ok_or_else(|| {
                talos_error::TalosError::new(
                    talos_error::ErrorCode::ResourceNotFound,
                    "no Agent routing binding matched the Session source",
                )
            })?;
        self.registry.get(&agent_id).await?;
        Ok(agent_id)
    }
}

/// Build the route channel used by Coordinator routing policy.
pub(crate) fn route_channel(source: &SessionSource) -> String {
    match source.channel_id.as_deref() {
        Some(channel_id) => format!("{}:{}", source.source_provider, channel_id),
        None => source.source_provider.to_string(),
    }
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
