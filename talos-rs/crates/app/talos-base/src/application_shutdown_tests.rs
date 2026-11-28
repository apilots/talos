use super::*;
use talos_app_server::BackgroundTaskDrain;

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
        service: runtime.service.clone(),
        automation: runtime.automation.clone(),
    };
    drain.drain(Duration::from_secs(5)).await.unwrap();
    runtime
        .shutdown()
        .await
        .expect("stdio cleanup must not shut down an already removed Harness");
    runtime.shutdown_host().await.unwrap();
    fs::remove_dir_all(workspace).unwrap();
}
