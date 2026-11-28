use super::*;
use talos_app_server::BackgroundTaskDrain;
use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot};

#[tokio::test]
async fn stdio_delivery_drain_then_runtime_shutdown_closes_the_worker_once() {
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "stdio-shutdown");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.llm.anthropic.models = vec![provider_model(&config.llm.model)];
    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .unwrap();
    let drain = crate::ProductDeliveryDrain {
        worker: runtime.worker.as_ref().unwrap().clone(),
        service: runtime.service.clone(),
        automation: runtime.automation.clone(),
    };
    drain.drain(Duration::from_secs(5)).await.unwrap();
    runtime
        .shutdown()
        .await
        .expect("stdio cleanup must not shut down an already removed Harness");
    runtime.shutdown_host().await.unwrap();
    runtime.auth_service.shutdown().await;
    drop(drain);
    drop(runtime);
    fs::remove_dir_all(workspace).unwrap();
}

#[tokio::test]
async fn runtime_shutdown_stops_local_execution_before_draining_product_facts() {
    use talos_store::session::AgentExecutionStore;
    use talos_types::{AgentUserContent, AgentUserInput, StartTurnRequest, UserPermissionMode};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    let (entered, running) = oneshot::channel();
    let upstream = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        entered.send(()).unwrap();
        std::future::pending::<()>().await;
        drop(socket);
    });
    let mut config = Config::default();
    let workspace = isolate_runtime_workspace(&mut config, "running-shutdown");
    config.gateway.auth = false;
    config.storage.connection_string = ":memory:".into();
    config.agent.kernel.plugins.sources.clear();
    config.agent.kernel.plugins.roots.clear();
    config.llm = serde_json::from_value(serde_json::json!({
        "provider":"shutdown-upstream","model":"blocked","custom_providers":{
            "shutdown-upstream":{"name":"Shutdown test","protocol":"chat","base_url":endpoint,"models":[{"id":"blocked"}]}
        }
    })).unwrap();
    fs::create_dir_all(config.talos_paths().conf_dir.clone()).unwrap();
    let mut persisted = serde_json::to_value(&config).unwrap();
    persisted["logging"].as_object_mut().unwrap().remove("file");
    persisted["storage"]
        .as_object_mut()
        .unwrap()
        .remove("connection_string");
    fs::write(
        config.source_path(),
        serde_json::to_vec(&persisted).unwrap(),
    )
    .unwrap();
    let runtime = build_application_runtime_with_migration_dir(config, &test_migrations_dir())
        .await
        .unwrap();
    let session_id = "shutdown-running-turn";
    runtime
        .gateway
        .start_turn(
            &talos_auth::Actor::auth_disabled(),
            talos_types::talos::SessionSource::http(),
            StartTurnRequest {
                node_id: None,
                session_id: session_id.into(),
                input: AgentUserInput {
                    content: vec![AgentUserContent::Text {
                        text: "Keep running until the Worker is stopped.".into(),
                    }],
                },
                model_selection: None,
                user_permission_mode: Some(UserPermissionMode::FullAccess),
                project_id: None,
                current_directory: None,
                client_request_id: Some("shutdown-request".into()),
            },
            None,
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    assert!(runtime
        .host
        .stores
        .sessions
        .get_agent_active_turn(session_id)
        .await
        .unwrap()
        .is_some());
    let stopped = tokio::time::timeout(Duration::from_secs(5), runtime.shutdown()).await;
    upstream.abort();
    let _ = upstream.await;
    stopped
        .expect("local execution must settle before product delivery is drained")
        .unwrap();
    let terminal = runtime
        .worker
        .as_ref()
        .unwrap()
        .execution_store()
        .terminal_events(session_id, None, 10)
        .await
        .unwrap();
    assert_eq!(terminal.len(), 1);
    assert!(
        matches!(&terminal[0].1.payload, talos_agent_api::AgentEventPayload::TurnFailed(failure) if failure.error.code == talos_error::ErrorCode::Cancelled)
    );
    assert!(runtime
        .host
        .stores
        .sessions
        .get_agent_active_turn(session_id)
        .await
        .unwrap()
        .is_none());
    assert!(runtime
        .host
        .stores
        .sessions
        .pending_worker_settlements(session_id)
        .await
        .unwrap()
        .is_empty());
    runtime.shutdown_host().await.unwrap();
    runtime.auth_service.shutdown().await;
    drop(runtime);
    fs::remove_dir_all(workspace).unwrap();
}
