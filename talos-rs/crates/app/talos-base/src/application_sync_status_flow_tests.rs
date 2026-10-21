//! Cached-login document transport must restore even while Gateway TLS is stalled.
use super::*;

pub(super) async fn verify(
    mut config: Config,
    migration: &Path,
    actor: &talos_service::Actor,
    scope: &talos_sync::SyncScope,
) {
    let runtime = build_application_runtime_with_migration_dir(config.clone(), migration)
        .await
        .unwrap();
    let identity = runtime.sync_host.verified_actor(actor).await.unwrap();
    runtime
        .start_account_sync(
            &identity,
            talos_sync::SyncBindParams {
                scope: scope.clone(),
                source_user_id: actor.user_id.clone(),
                computers: vec![],
            },
            Arc::new(|| {
                Box::pin(async {
                    Ok(talos_sync_sdk::VerifiedCredential {
                        account_id: ACCOUNT.into(),
                        access_token: TOKEN.into(),
                    })
                })
            }),
        )
        .await
        .unwrap();
    close(runtime).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    config.cloud.agent_base_url = format!("https://{}", listener.local_addr().unwrap());
    std::fs::write(config.source_path(), persisted_config(&config)).unwrap();
    let stalled_gateway = tokio::spawn(async move {
        let mut connections = Vec::new();
        loop {
            connections.push(listener.accept().await.unwrap());
        }
    });
    let now = talos_auth::CloudSessionCache::now_ms();
    talos_auth::CloudSessionCache::for_talos_home(config.talos_paths().talos_home)
        .store(&talos_auth::CloudSessionCacheEntry {
            schema_version: 2,
            cloud_api_base_url: config.cloud.api_base_url.clone(),
            access_token: TOKEN.into(),
            token_type: "Bearer".into(),
            expires_at_ms: Some(now + 60_000),
            last_used_at_ms: now,
            user: talos_auth::CloudSessionUserSnapshot {
                user_id: actor.user_id.clone(),
                username: "fixture".into(),
                email: None,
                role: talos_auth::UserRole::User,
                r#type: talos_auth::UserType::Free,
            },
        })
        .unwrap();
    let restored = build_application_runtime_with_migration_dir(config, migration)
        .await
        .unwrap();
    // No status query, enrollment request or UI poll is needed to initialize H.
    tokio::time::timeout(Duration::from_secs(5), async {
        wait_for("cached-login Sync independent of Gateway", async || {
            restored
                .sync_host
                .sync_stores
                .handles
                .lock()
                .await
                .get(ACCOUNT)
                .is_some_and(|handle| handle.status().connection == SyncConnectionState::Connected)
        })
        .await;
    })
    .await
    .expect("Gateway TLS must not delay saved document transport");
    let handle = restored.sync_host.sync_stores.handles.lock().await[ACCOUNT].clone();
    assert!(restored.sync_host.lifecycle.try_lock().is_err());
    for _ in 0..5 {
        let status =
            tokio::time::timeout(Duration::from_secs(1), restored.service.sync_status(actor))
                .await
                .expect("status must observe H while Worker registration is stalled")
                .unwrap();
        assert_eq!(status.connection, SyncConnectionState::Connected);
        restored.service.resume_account_sync(actor);
        assert!(Arc::ptr_eq(
            &handle,
            &restored.sync_host.sync_stores.handles.lock().await[ACCOUNT]
        ));
    }
    tokio::time::timeout(Duration::from_secs(5), close(restored))
        .await
        .expect("shutdown must cancel stalled account initialization");
    stalled_gateway.abort();
}
