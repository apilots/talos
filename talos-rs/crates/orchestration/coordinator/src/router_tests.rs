use std::sync::Arc;

use async_trait::async_trait;
use talos_error::TalosResult;
use talos_router::Binding;
use talos_types::talos::{SessionSource, SessionSourceProvider, SessionSourceType};

use super::*;
use crate::test_support::TestHarness;

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

fn test_source() -> SessionSource {
    SessionSource {
        source_type: SessionSourceType::Http,
        source_provider: SessionSourceProvider::Sdk,
        channel_id: Some("channel-a".to_string()),
        thread_id: None,
        environment: None,
    }
}

fn test_router(policy_target: Option<&str>) -> (AgentRouter, AgentRegistry) {
    let registry = AgentRegistry::new();
    let router = AgentRouter::new(
        Arc::new(StaticPolicyRouter {
            target_agent_id: policy_target.map(str::to_string),
        }),
        registry.clone(),
    );
    (router, registry)
}

async fn register_agent(registry: &AgentRegistry, id: &str) {
    registry
        .register(id.to_string(), Arc::new(TestHarness))
        .await
        .expect("agent should register");
}

#[tokio::test]
async fn router_resolves_a_registered_routing_target() {
    let (router, registry) = test_router(Some("reviewer"));
    register_agent(&registry, "reviewer").await;

    let selection = router
        .resolve_agent(AgentSelectionParams::for_session_source(
            test_source(),
            "user-1".to_string(),
        ))
        .await
        .expect("policy target should resolve");

    assert_eq!(selection, "reviewer");
}

#[tokio::test]
async fn router_rejects_a_missing_composition_root_binding() {
    let (router, registry) = test_router(None);
    register_agent(&registry, "default").await;
    register_agent(&registry, "reviewer").await;

    let error = router
        .resolve_agent(AgentSelectionParams::for_session_source(
            test_source(),
            "user-1".to_string(),
        ))
        .await
        .expect_err("Coordinator must not invent a fallback Agent");

    assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
}
