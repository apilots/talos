use std::sync::Arc;

use talos_agent::MockAgent;

use super::AgentRegistry;

#[tokio::test]
async fn registry_stores_harness_and_descriptor() {
    let registry = AgentRegistry::new();

    registry
        .register("mock", Arc::new(MockAgent::new()))
        .await
        .expect("agent should register");

    let registration = registry.get("mock").await.expect("agent should exist");
    assert_eq!(registration.descriptor.agent_id, "mock");
    assert_eq!(registration.harness.id(), "mock");
    assert_eq!(registry.len().await, 1);
}

#[tokio::test]
async fn registry_rejects_empty_agent_id() {
    let registry = AgentRegistry::new();

    let error = registry
        .register("", Arc::new(MockAgent::new()))
        .await
        .expect_err("empty id should fail");

    assert!(error.to_string().contains("agent_id"));
}

#[tokio::test]
async fn registry_rejects_descriptor_id_mismatch() {
    let registry = AgentRegistry::new();

    let error = registry
        .register("alias", Arc::new(MockAgent::new()))
        .await
        .expect_err("registry aliases should fail");

    assert!(error.to_string().contains("does not match descriptor id"));
}

#[tokio::test]
async fn registry_rejects_duplicate_agent_id() {
    let registry = AgentRegistry::new();
    registry
        .register("mock", Arc::new(MockAgent::new()))
        .await
        .expect("first registration should succeed");

    let error = registry
        .register("mock", Arc::new(MockAgent::new()))
        .await
        .expect_err("duplicate registration should fail");

    assert!(error.to_string().contains("already registered"));
}
