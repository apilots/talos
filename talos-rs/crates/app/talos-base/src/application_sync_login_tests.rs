//! Actual first-login enrollment, explicit opt-out, durable capture and restart recovery.
use super::*;
pub(super) async fn verify(
    fresh_config: Config,
    migration: &Path,
    actor: &talos_service::Actor,
    gateway: &agent_client::AgentNodeClient,
) -> TalosApplicationRuntime {
    let fresh = build_application_runtime_with_migration_dir(fresh_config, migration)
        .await
        .unwrap();
    // No hub association exists yet. Consent must persist without discovery.
    let before_config = std::fs::read(fresh.config.source_path()).unwrap();
    let disabled = fresh.service.disable_sync(actor).await.unwrap();
    assert_eq!(disabled.configured_enabled, Some(false));
    assert!(fresh.sync.saved_bindings().await.unwrap().is_empty());
    assert_eq!(
        std::fs::read(fresh.config.source_path()).unwrap(),
        before_config
    );
    let restarted_config = fresh.config.reload().unwrap();
    close(fresh).await;
    let fresh = build_application_runtime_with_migration_dir(restarted_config, migration)
        .await
        .unwrap();
    let disabled = fresh.service.sync_status(actor).await.unwrap();
    assert_eq!(disabled.configured_enabled, Some(false));
    assert_eq!(
        disabled.connection,
        talos_types::SyncConnectionState::Disabled
    );
    assert!(fresh.sync.saved_bindings().await.unwrap().is_empty());
    fresh
        .service
        .enable_sync(actor, Default::default())
        .await
        .unwrap();
    let fresh_id = fresh
        .service
        .sync_status(actor)
        .await
        .unwrap()
        .local_computer_id
        .expect("owned local Computer");
    assert!(uuid::Uuid::parse_str(&fresh_id).is_ok());
    wait_for("login Worker available", async || {
        gateway
            .get_node(TOKEN, &fresh_id)
            .await
            .is_ok_and(|node| node.available)
    })
    .await;
    let before_config = std::fs::read(fresh.config.source_path()).unwrap();
    fresh.service.disable_sync(actor).await.unwrap();
    assert_eq!(
        std::fs::read(fresh.config.source_path()).unwrap(),
        before_config
    );
    fresh
        .service
        .start_turn(
            actor,
            talos_types::talos::SessionSource::http(),
            start_request("login-history", "before host restart"),
            None,
        )
        .await
        .unwrap();
    wait_for("login history settled", async || {
        fresh
            .service
            .get_active_turn(actor, "login-history")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    let restarted_config = fresh.config.reload().unwrap();
    close(fresh).await;
    let fresh = build_application_runtime_with_migration_dir(restarted_config, migration)
        .await
        .unwrap();
    let resumed = fresh.service.sync_status(actor).await.unwrap();
    assert_eq!(
        resumed.local_computer_id.as_deref(),
        Some(fresh_id.as_str())
    );
    assert_eq!(
        resumed.connection,
        talos_types::SyncConnectionState::Disabled
    );
    assert!(resumed.pending_changes > 0);
    fresh
        .service
        .start_turn(
            actor,
            talos_types::talos::SessionSource::http(),
            start_request("login-history", "after host restart"),
            None,
        )
        .await
        .unwrap();
    wait_for("restarted login history settled", async || {
        fresh
            .service
            .get_active_turn(actor, "login-history")
            .await
            .is_ok_and(|state| state.active_turn.is_none())
    })
    .await;
    fresh
        .service
        .enable_sync(actor, Default::default())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(fresh.config.source_path()).unwrap(),
        before_config
    );
    wait_for("resumed sync connected", async || {
        fresh.service.sync_status(actor).await.unwrap().connection
            == talos_types::SyncConnectionState::Connected
    })
    .await;

    // Host operator configuration is independent of the account's product switch.
    talos_config::YamlConfigStore::new(fresh.config.source_path())
        .save_sync_enabled(false)
        .unwrap();
    let operator_config = std::fs::read(fresh.config.source_path()).unwrap();
    let denied = fresh
        .service
        .enable_sync(actor, Default::default())
        .await
        .unwrap_err();
    assert_eq!(
        denied.error_code(),
        talos_error::ErrorCode::PermissionDenied
    );
    assert_eq!(
        std::fs::read(fresh.config.source_path()).unwrap(),
        operator_config
    );
    let status = fresh.service.sync_status(actor).await.unwrap();
    assert_eq!(
        status.connection,
        talos_types::SyncConnectionState::Disabled
    );
    assert!(fresh.sync_stores.handles.lock().await.is_empty());

    fresh
}
