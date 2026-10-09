use std::sync::Arc;

use super::AgentRegistry;
use crate::test_support::TestHarness;

#[tokio::test]
async fn registry_rejects_empty_agent_id() {
    let registry = AgentRegistry::new();

    let error = registry
        .register("embedded".into(), String::new(), Arc::new(TestHarness))
        .await
        .expect_err("empty id should fail");

    assert!(error.to_string().contains("agent_id"));
}

#[tokio::test]
async fn registry_rejects_duplicate_agent_id() {
    let registry = AgentRegistry::new();
    registry
        .register("embedded".into(), "mock".to_string(), Arc::new(TestHarness))
        .await
        .expect("first registration should succeed");

    let error = registry
        .register("embedded".into(), "mock".to_string(), Arc::new(TestHarness))
        .await
        .expect_err("duplicate registration should fail");

    assert!(error.to_string().contains("already registered"));
}
