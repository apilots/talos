use super::*;
use async_trait::async_trait;
use std::sync::Arc;
use std::time::Duration;
use talos_agent::{
    AgentCapabilities, AgentDescriptor, AgentHarness, AgentInputParams, AgentInputResult,
    AgentInterruptParams, AgentInterruptResult, AgentKind, AgentResult, AgentRunParams,
    AgentRunResult, AgentStatus,
};
use talos_error::TalosResult;
use talos_router::Binding;
use talos_types::talos::{
    AgentEvent, StopReason, TalosContext, TalosProvider, TalosSourceType, TokenUsage,
};
use talos_types::TalosSession;
use tokio::sync::mpsc;

#[derive(Clone)]
struct StaticPolicyRouter {
    target_agent_id: Option<String>,
}

#[async_trait]
impl talos_router::Router for StaticPolicyRouter {
    async fn route(&self, _channel: &str, _user_id: &str) -> TalosResult<Option<String>> {
        Ok(self.target_agent_id.clone())
    }

    async fn add_binding(&self, _binding: Binding) -> TalosResult<()> {
        Ok(())
    }

    async fn remove_binding(&self, _id: &str) -> TalosResult<()> {
        Ok(())
    }

    async fn list_bindings(&self) -> TalosResult<Vec<Binding>> {
        Ok(Vec::new())
    }
}

struct DescriptorAgent {
    id: String,
    name: String,
    status: AgentStatus,
}

impl DescriptorAgent {
    fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: AgentStatus::Enabled,
        }
    }

    fn disabled(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: AgentStatus::Disabled,
        }
    }
}

#[async_trait]
impl AgentHarness for DescriptorAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            agent_id: self.id.clone(),
            name: self.name.clone(),
            kind: AgentKind::Kernel,
            description: String::new(),
            capabilities: AgentCapabilities::kernel(),
            status: self.status.clone(),
        }
    }

    async fn run(&self, request: AgentRunParams) -> AgentResult<AgentRunResult> {
        Ok(AgentRunResult {
            output_text: String::new(),
            output_items: Vec::new(),
            reasoning: Default::default(),
            stop_reason: StopReason::EndTurn,
            usage: TokenUsage::default(),
            task_count: 0,
            session_id: request.context.session_id,
            generated_session_title: None,
        })
    }

    async fn run_streamed(
        &self,
        _request: AgentRunParams,
    ) -> AgentResult<mpsc::Receiver<AgentEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(rx)
    }

    async fn compact_session(
        &self,
        _params: talos_agent::AgentSessionCompactionParams,
    ) -> TalosResult<TokenUsage> {
        Ok(TokenUsage::default())
    }

    async fn clear_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    async fn close_session(&self, _session: TalosSession) -> TalosResult<()> {
        Ok(())
    }

    fn has_active_turn(&self, _session_id: &str) -> bool {
        false
    }

    async fn interrupt(&self, _params: AgentInterruptParams) -> AgentResult<AgentInterruptResult> {
        Ok(AgentInterruptResult { interrupted: true })
    }

    async fn drain_active_turns(&self, _timeout: Duration) -> TalosResult<()> {
        Ok(())
    }

    async fn submit_input(&self, _params: AgentInputParams) -> AgentResult<AgentInputResult> {
        Ok(AgentInputResult { accepted: true })
    }
}

fn test_context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::Http,
        provider: TalosProvider::Sdk,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: Some("channel-a".to_string()),
        thread_id: None,
        project_root: None,
        environment: None,
    }
}

async fn test_router(policy_target: Option<&str>) -> (AgentRouter, AgentRegistry) {
    let registry = AgentRegistry::new();
    let router = AgentRouter::new(
        Arc::new(StaticPolicyRouter {
            target_agent_id: policy_target.map(str::to_string),
        }),
        registry.clone(),
        DEFAULT_AGENT_ID,
    );
    (router, registry)
}

async fn register_agent(registry: &AgentRegistry, id: &str, name: &str) {
    registry
        .register(id, Arc::new(DescriptorAgent::new(id, name)))
        .await
        .expect("agent should register");
}

#[tokio::test]
async fn router_uses_routing_policy_before_default() {
    let (router, registry) = test_router(Some("reviewer")).await;
    register_agent(&registry, DEFAULT_AGENT_ID, "Default").await;
    register_agent(&registry, "reviewer", "Reviewer").await;

    let selection = router
        .resolve_agent(AgentSelectionParams::for_context(test_context()))
        .await
        .expect("policy target should resolve");

    assert_eq!(selection.agent_id, "reviewer");
    assert_eq!(selection.source, AgentSelectionSource::RoutingPolicy);
}

#[tokio::test]
async fn router_uses_configured_default_when_policy_has_no_match() {
    let (router, registry) = test_router(None).await;
    register_agent(&registry, DEFAULT_AGENT_ID, "Default").await;
    register_agent(&registry, "reviewer", "Reviewer").await;

    let selection = router
        .resolve_agent(AgentSelectionParams::for_context(test_context()))
        .await
        .expect("default target should resolve");

    assert_eq!(selection.agent_id, DEFAULT_AGENT_ID);
    assert_eq!(selection.source, AgentSelectionSource::DefaultAgent);
}

#[tokio::test]
async fn router_prefers_internal_explicit_id_over_policy() {
    let (router, registry) = test_router(Some("reviewer")).await;
    register_agent(&registry, DEFAULT_AGENT_ID, "Default").await;
    register_agent(&registry, "reviewer", "Reviewer").await;
    register_agent(&registry, "specialist", "Specialist").await;

    let selection = router
        .resolve_agent(
            AgentSelectionParams::for_context(test_context()).with_explicit_agent_id("specialist"),
        )
        .await
        .expect("explicit id should resolve");

    assert_eq!(selection.agent_id, "specialist");
    assert_eq!(selection.source, AgentSelectionSource::ExplicitAgentId);
}

#[tokio::test]
async fn router_resolves_internal_explicit_name() {
    let (router, registry) = test_router(Some("reviewer")).await;
    register_agent(&registry, DEFAULT_AGENT_ID, "Default").await;
    register_agent(&registry, "reviewer", "Reviewer").await;
    register_agent(&registry, "specialist", "Specialist").await;

    let selection = router
        .resolve_agent(
            AgentSelectionParams::for_context(test_context())
                .with_explicit_agent_name("Specialist"),
        )
        .await
        .expect("explicit name should resolve");

    assert_eq!(selection.agent_id, "specialist");
    assert_eq!(selection.source, AgentSelectionSource::ExplicitAgentName);
}

#[tokio::test]
async fn router_rejects_disabled_agent_selection() {
    let (router, registry) = test_router(Some("disabled")).await;
    registry
        .register(
            "disabled",
            Arc::new(DescriptorAgent::disabled("disabled", "Disabled")),
        )
        .await
        .expect("disabled descriptor can be registered");

    let result = router
        .resolve_agent(AgentSelectionParams::for_context(test_context()))
        .await;
    let Err(error) = result else {
        panic!("disabled agent should be rejected");
    };

    assert!(error.to_string().contains("disabled"));
}
