//! Explicit re-enrollment restores capture for local work created after retirement.
use super::*;

#[tokio::test]
async fn fresh_enrollment_publishes_post_removal_local_history_without_rebinding_old_facts() {
    run_topology(TopologyScenario::FreshEnrollment).await;
}

pub(super) enum TopologyScenario {
    SharedSession,
    FreshEnrollment,
    LateEnrollment,
}

pub(super) async fn verify(
    config: Config,
    migration: &Path,
    actor: &talos_service::Actor,
    gateway: &agent_client::AgentNodeClient,
    target: &TalosApplicationRuntime,
) {
    target
        .service
        .workspace_info(
            actor,
            talos_types::WorkspaceInfoRequest {
                node_id: Some(
                    target
                        .config
                        .agent_node
                        .runner
                        .as_ref()
                        .unwrap()
                        .node_id
                        .clone(),
                ),
            },
        )
        .await
        .unwrap();
    target.service.resume_account_host(actor);
    wait_for("enrollment receiver document sync", async || {
        target.service.sync_status(actor).await.unwrap().connection
            == SyncConnectionState::Connected
    })
    .await;
    let cache = talos_auth::CloudSessionCache::for_talos_home(config.talos_paths().talos_home);
    let now = talos_auth::CloudSessionCache::now_ms();
    cache
        .store(&talos_auth::CloudSessionCacheEntry {
            schema_version: 2,
            cloud_api_base_url: config.cloud.api_base_url.clone(),
            access_token: TOKEN.into(),
            token_type: "Bearer".into(),
            expires_at_ms: Some(now + 120_000),
            last_used_at_ms: now,
            user: talos_auth::CloudSessionUserSnapshot {
                user_id: ACCOUNT.into(),
                username: ACCOUNT.into(),
                email: None,
                role: talos_auth::UserRole::User,
                r#type: talos_auth::UserType::Free,
            },
        })
        .unwrap();
    let source = build_application_runtime_with_migration_dir(config, migration)
        .await
        .unwrap();
    source
        .service
        .account
        .set_cloud_session_cache(cache)
        .unwrap();
    source
        .service
        .enable_sync(actor, Default::default())
        .await
        .unwrap();
    wait_for("source Worker enrolled", async || {
        source
            .service
            .sync_status(actor)
            .await
            .unwrap()
            .local_computer_id
            .is_some()
    })
    .await;
    let identity = source.host.verified_actor(actor).await.unwrap();
    let desktop = crate::desktop_application::DesktopApplication::new(
        &source,
        "Fresh capture regression".into(),
        Arc::new(talos_app_server::TalosApplicationAdapter::new(
            source.service.clone(),
            128,
        )),
        talos_app_server::DesktopHostBridge::new(),
    );
    let old = desktop
        .enroll_worker(None, Some(actor))
        .await
        .unwrap()
        .node_id
        .unwrap();
    wait_for("fresh capture original Worker", async || {
        gateway
            .get_node(TOKEN, &old)
            .await
            .is_ok_and(|node| node.available)
    })
    .await;
    source
        .service
        .start_turn(
            actor,
            talos_types::talos::SessionSource::http(),
            start_request("enrollment-old", "before retirement"),
            None,
        )
        .await
        .unwrap();
    let request = |id: &str| talos_types::SessionHistoryRequest {
        session_id: id.into(),
        cursor: None,
        limit: None,
    };
    wait_for("original local history shared", async || {
        target
            .service
            .get_session_history(actor, request("enrollment-old"))
            .await
            .is_ok_and(|history| history.data.len() == 1)
    })
    .await;
    let original = target
        .service
        .get_session_history(actor, request("enrollment-old"))
        .await
        .unwrap();
    gateway
        .delete_computer(
            TOKEN,
            talos_types::DeleteComputerRequest {
                node_id: old.clone(),
            },
        )
        .await
        .unwrap();
    source
        .host
        .observe_registration_removal(&identity, &old)
        .await
        .unwrap();
    assert!(source
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap()
        .computers
        .is_empty());
    source
        .service
        .start_turn(
            actor,
            talos_types::talos::SessionSource::http(),
            start_request("enrollment-new", "after retirement"),
            None,
        )
        .await
        .unwrap();
    wait_for("unregistered local history settled", async || {
        source
            .service
            .get_session_history(actor, request("enrollment-new"))
            .await
            .is_ok_and(|history| history.data.len() == 1)
    })
    .await;
    let local = source
        .service
        .get_session_history(actor, request("enrollment-new"))
        .await
        .unwrap();
    assert!(target
        .session_store
        .get_session_record("enrollment-new")
        .await
        .unwrap()
        .is_none());
    let other = talos_service::Actor::regular("other-account").with_access_token(OTHER_TOKEN);
    source.host.refresh_enrolled_capture(&other).await.unwrap();
    assert!(source
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap()
        .computers
        .is_empty());
    let pending_uuid = verify_suspend_interleaving(&source, &desktop, actor).await;
    source.service.resume_account_host_after_login(actor);
    let fresh = desktop
        .enroll_worker(None, Some(actor))
        .await
        .unwrap()
        .node_id
        .unwrap();
    assert_ne!(fresh, old);
    assert_eq!(fresh, pending_uuid);
    let binding = source
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        binding.computers,
        vec![talos_sync::ComputerAlias {
            source_node_id: "embedded".into(),
            computer_id: fresh.clone()
        }]
    );
    assert_eq!(binding.source_user_id, ACCOUNT);
    assert_eq!(binding.scope.account_id, ACCOUNT);
    assert_eq!(binding.peer_id, source.sync.peer_id());
    wait_for("post-removal local history bootstrapped", async || {
        target
            .service
            .get_session_history(actor, request("enrollment-new"))
            .await
            .is_ok_and(|history| {
                if history.data.is_empty() {
                    return false;
                }
                let mut actual = history.data;
                let mut expected = local.data.clone();
                // These cursors identify each database's local materialization.
                // Compare every canonical execution and notice field across peers.
                for turn in actual.iter_mut().chain(expected.iter_mut()) {
                    turn.completed_event_seq = 0;
                    for notice in &mut turn.notices {
                        notice.event_id.clear();
                        notice.event_seq = 0;
                    }
                }
                assert_eq!(actual, expected);
                true
            })
    })
    .await;
    assert_eq!(
        target
            .service
            .get_session(actor, "enrollment-new")
            .await
            .unwrap()
            .node_id,
        fresh
    );
    assert_eq!(
        target
            .service
            .get_session(actor, "enrollment-old")
            .await
            .unwrap()
            .node_id,
        old
    );
    assert_eq!(
        target
            .service
            .get_session_history(actor, request("enrollment-old"))
            .await
            .unwrap(),
        original
    );
    assert_eq!(
        source
            .session_store
            .get_session_record("enrollment-old")
            .await
            .unwrap()
            .unwrap()
            .execution_node_id,
        old
    );
    // Explicit enrollment preserves an account opt-out while restoring capture.
    gateway
        .delete_computer(
            TOKEN,
            talos_types::DeleteComputerRequest {
                node_id: fresh.clone(),
            },
        )
        .await
        .unwrap();
    source
        .host
        .observe_registration_removal(&identity, &fresh)
        .await
        .unwrap();
    source.service.disable_sync(actor).await.unwrap();
    let disabled_uuid = desktop
        .enroll_worker(None, Some(actor))
        .await
        .unwrap()
        .node_id
        .unwrap();
    assert_ne!(disabled_uuid, fresh);
    let disabled = source.service.sync_status(actor).await.unwrap();
    assert_eq!(disabled.configured_enabled, Some(false));
    assert_eq!(disabled.connection, SyncConnectionState::Disabled);
    assert!(!source
        .host
        .stores
        .handles
        .lock()
        .await
        .contains_key(&actor.user_id));
    assert_eq!(
        source
            .sync
            .account_binding(&identity)
            .await
            .unwrap()
            .unwrap()
            .computers,
        vec![talos_sync::ComputerAlias {
            source_node_id: "embedded".into(),
            computer_id: disabled_uuid
        }]
    );
    drop(desktop);
    close(source).await;
}

async fn verify_suspend_interleaving(
    source: &TalosApplicationRuntime,
    desktop: &crate::desktop_application::DesktopApplication,
    actor: &talos_service::Actor,
) -> String {
    // Finish Worker enrollment while leaving the capture reconciliation in flight.
    let fresh = crate::desktop_worker::WorkerEnrollment {
        host: &source.host,
        config: &desktop.config,
        store: &desktop.store,
        account: &desktop.account,
        auth: &desktop.auth,
        worker: &desktop.worker,
        execution: &desktop.execution,
        rpc: &desktop.rpc,
        device_name: &desktop.device_name,
        worker_failures: &desktop.worker_failures,
    }
    .enroll_worker(None, None)
    .await
    .unwrap()
    .node_id
    .unwrap();
    let refreshed =
        talos_service::Actor::regular(&actor.user_id).with_access_token(REFRESHED_TOKEN);
    source
        .auth_service
        .verify_access_token(REFRESHED_TOKEN)
        .await
        .unwrap();
    let maintenance = source.host.suspend_refresh(actor).await;
    let lifecycle = source.host.lifecycle.lock().await;
    let mut suspend = std::pin::pin!(source.service.suspend_account_sync(actor));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(suspend.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    let mut capture = std::pin::pin!(source.host.refresh_enrolled_capture(&refreshed));
    std::future::poll_fn(|context| {
        assert!(std::future::Future::poll(capture.as_mut(), context).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    assert!(!source.host.account_connections_allowed(&actor.user_id));
    drop(lifecycle);
    // Complete pre-suspend before allowing the queued enrollment to make progress.
    tokio::time::timeout(Duration::from_secs(5), suspend)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), capture)
        .await
        .unwrap()
        .unwrap();
    assert!(!source
        .host
        .stores
        .handles
        .lock()
        .await
        .contains_key(&actor.user_id));
    assert!(!source
        .host
        .stores
        .credentials
        .read()
        .unwrap()
        .contains_key(&actor.user_id));
    #[cfg(feature = "relay")]
    assert!(source.host.product_relay.lock().await.is_none());
    assert!(source
        .sync
        .saved_bindings()
        .await
        .unwrap()
        .iter()
        .all(|binding| binding.computers.is_empty()));
    drop(maintenance);
    // A new credential must not let a delayed completion undo pre-suspend.
    source
        .host
        .refresh_enrolled_capture(&refreshed)
        .await
        .unwrap();
    assert!(!source
        .host
        .stores
        .handles
        .lock()
        .await
        .contains_key(&actor.user_id));
    assert!(!source.host.account_connections_allowed(&actor.user_id));
    fresh
}
