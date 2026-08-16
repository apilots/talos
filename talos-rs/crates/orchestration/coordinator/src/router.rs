use std::sync::Arc;

use talos_agent::{AgentError, AgentErrorKind, AgentStatus};
use talos_error::{TalosError, TalosResult};
use talos_router::Router;
use talos_types::talos::TalosContext;

use crate::{AgentRegistration, AgentRegistry};

pub(crate) const DEFAULT_AGENT_ID: &str = "default";

/// Parameters used by Coordinator to select an Agent.
///
/// This structure is an internal orchestration input, not a Gateway or SDK
/// request DTO. External product APIs should not expose these fields until the
/// multi-Agent product selection model is explicitly designed.
#[derive(Debug, Clone)]
pub struct AgentSelectionParams {
    /// Product context used by routing policy.
    pub context: TalosContext,
    /// Internal explicit Agent id selected by trusted Coordinator logic.
    pub explicit_agent_id: Option<String>,
    /// Internal explicit Agent name selected by trusted Coordinator logic.
    pub explicit_agent_name: Option<String>,
}

impl AgentSelectionParams {
    /// Build selection params from product context with no explicit override.
    pub fn for_context(context: TalosContext) -> Self {
        Self {
            context,
            explicit_agent_id: None,
            explicit_agent_name: None,
        }
    }

    /// Attach an internal explicit Agent id override.
    pub fn with_explicit_agent_id(mut self, agent_id: impl Into<String>) -> Self {
        self.explicit_agent_id = Some(agent_id.into());
        self
    }

    /// Attach an internal explicit Agent name override.
    pub fn with_explicit_agent_name(mut self, agent_name: impl Into<String>) -> Self {
        self.explicit_agent_name = Some(agent_name.into());
        self
    }
}

/// Source that produced an Agent selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSelectionSource {
    /// Selection came from an internal explicit Agent id.
    ExplicitAgentId,
    /// Selection came from an internal explicit Agent name.
    ExplicitAgentName,
    /// Selection came from the Coordinator routing policy.
    RoutingPolicy,
    /// Selection fell back to the configured default Agent.
    DefaultAgent,
}

/// Result of resolving an Agent for Coordinator dispatch.
#[derive(Clone)]
pub struct AgentSelectionResult {
    /// Selected Agent id.
    pub agent_id: String,
    /// Registered Harness and descriptor for the selected Agent.
    pub registration: AgentRegistration,
    /// Selection source used for diagnostics and tests.
    pub source: AgentSelectionSource,
}

/// Coordinator-owned Agent selection framework.
///
/// AgentRouter composes internal explicit selection, product routing policy,
/// configured default selection, and registry validation. It must not parse
/// runtime-private config or use workspace_id as a direct Agent selector.
#[derive(Clone)]
pub struct AgentRouter {
    routing_policy: Arc<dyn Router>,
    registry: AgentRegistry,
    default_agent_id: String,
}

impl AgentRouter {
    /// Build a Coordinator Agent router.
    pub fn new(
        routing_policy: Arc<dyn Router>,
        registry: AgentRegistry,
        default_agent_id: impl Into<String>,
    ) -> Self {
        Self {
            routing_policy,
            registry,
            default_agent_id: default_agent_id.into(),
        }
    }

    /// Resolve an Agent registration using the standard Coordinator priority.
    ///
    /// Priority is internal explicit id, internal explicit name, routing
    /// policy, then configured default Agent. workspace_id is intentionally not
    /// read here; workspace-based routing must be expressed as a routing policy
    /// or session binding before reaching this boundary.
    pub async fn resolve_agent(
        &self,
        params: AgentSelectionParams,
    ) -> TalosResult<AgentSelectionResult> {
        if let Some(agent_id) = normalized(params.explicit_agent_id.as_deref()) {
            return self
                .registration_by_id(agent_id, AgentSelectionSource::ExplicitAgentId)
                .await;
        }

        if let Some(agent_name) = normalized(params.explicit_agent_name.as_deref()) {
            return self.registration_by_name(agent_name).await;
        }

        let route_channel = route_channel(&params.context);
        if let Some(agent_id) = self
            .routing_policy
            .route(&route_channel, &params.context.user_id)
            .await?
            .and_then(|agent_id| normalized(Some(agent_id.as_str())).map(str::to_string))
        {
            return self
                .registration_by_id(&agent_id, AgentSelectionSource::RoutingPolicy)
                .await;
        }

        let default_agent_id = self.default_agent_id.clone();
        self.registration_by_id(&default_agent_id, AgentSelectionSource::DefaultAgent)
            .await
    }

    async fn registration_by_id(
        &self,
        agent_id: &str,
        source: AgentSelectionSource,
    ) -> TalosResult<AgentSelectionResult> {
        let registration = self
            .registry
            .get(agent_id)
            .await
            .map_err(AgentError::into_talos_error)?;
        ensure_enabled(&registration)?;
        Ok(AgentSelectionResult {
            agent_id: registration.descriptor.agent_id.clone(),
            registration,
            source,
        })
    }

    async fn registration_by_name(&self, agent_name: &str) -> TalosResult<AgentSelectionResult> {
        let matches = self
            .registry
            .list()
            .await
            .into_iter()
            .filter(|registration| registration.descriptor.name == agent_name)
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [registration] => {
                ensure_enabled(registration)?;
                Ok(AgentSelectionResult {
                    agent_id: registration.descriptor.agent_id.clone(),
                    registration: registration.clone(),
                    source: AgentSelectionSource::ExplicitAgentName,
                })
            }
            [] => Err(
                AgentError::new(AgentErrorKind::AgentNotFound, agent_name.to_string())
                    .into_talos_error(),
            ),
            _ => Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("agent name `{agent_name}` matches multiple registered agents"),
            )),
        }
    }
}

/// Build the route channel used by Coordinator routing policy.
pub(crate) fn route_channel(context: &TalosContext) -> String {
    match context.channel_id.as_deref() {
        Some(channel_id) => format!("{}:{}", context.provider, channel_id),
        None => context.provider.to_string(),
    }
}

fn ensure_enabled(registration: &AgentRegistration) -> TalosResult<()> {
    if registration.descriptor.status == AgentStatus::Disabled {
        return Err(AgentError::new(
            AgentErrorKind::AgentDisabled,
            registration.descriptor.agent_id.clone(),
        )
        .into_talos_error());
    }
    Ok(())
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
