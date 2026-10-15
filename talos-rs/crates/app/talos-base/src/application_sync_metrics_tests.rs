//! Real Sync + management host + HTTP/RPC account-isolation acceptance.
use super::*;
use talos_app_server::{AppServerApplication, TalosApplicationAdapter};
use talos_error::ErrorCode;

pub(super) async fn verify_late_enrollment(
    config: Config,
    migration: &Path,
    actor: &talos_service::Actor,
    scope: &talos_sync::SyncScope,
) {
    let runtime = build_application_runtime_with_migration_dir(config, migration)
        .await
        .unwrap();
    let identity = RequestIdentity {
        user_id: ACCOUNT.into(),
        username: None,
        role: talos_auth::UserRole::User,
        auth_disabled: false,
        access_token: Some(TOKEN.into()),
    };
    // Reproduce a previous sign-in that discovered Sync before Worker enrollment.
    let handle = runtime
        .start_account_sync(
            &identity,
            talos_sync::SyncBindParams {
                scope: scope.clone(),
                source_user_id: ACCOUNT.into(),
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
    wait_for("pre-enrollment document sync", async || {
        handle.status().connection == talos_sync::SyncConnectionState::Connected
    })
    .await;
    assert!(runtime
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap()
        .computers
        .is_empty());
    let status = runtime.service.sync_status(actor).await.unwrap();
    let local = status
        .local_computer_id
        .expect("later enrollment must succeed");
    assert!(Arc::ptr_eq(
        &handle,
        runtime
            .sync_stores
            .handles
            .lock()
            .await
            .get(ACCOUNT)
            .unwrap()
    ));
    wait_for("late enrolled Computer presence", async || {
        runtime
            .service
            .sync_metrics(actor)
            .await
            .is_ok_and(|metrics| {
                metrics.peers.iter().any(|peer| {
                    peer.peer_id == runtime.sync.peer_id() && peer.computer_ids.contains(&local)
                })
            })
    })
    .await;
    // Observation refresh must not rewrite capture identity or restart execution.
    assert!(runtime
        .sync
        .account_binding(&identity)
        .await
        .unwrap()
        .unwrap()
        .computers
        .is_empty());
    assert_eq!(
        handle.status().connection,
        talos_sync::SyncConnectionState::Connected
    );
    close(runtime).await;
}

pub(super) async fn verify(
    a: &TalosApplicationRuntime,
    b: &TalosApplicationRuntime,
    actor: &talos_service::Actor,
    gateway_http_port: u16,
) {
    let a_id = &a.config.agent_node.runner.as_ref().unwrap().node_id;
    let b_id = &b.config.agent_node.runner.as_ref().unwrap().node_id;
    wait_for("authenticated Computer presence observations", async || {
        a.service.sync_metrics(actor).await.is_ok_and(|metrics| {
            [a_id, b_id].iter().all(|id| {
                metrics
                    .peers
                    .iter()
                    .any(|peer| peer.computer_ids.contains(id))
            })
        })
    })
    .await;
    let bindings = a.sync.saved_bindings().await.unwrap().len();
    let observations = a.service.sync_metrics(actor).await.unwrap();
    assert!(observations.observed_at < 10_000_000_000);
    assert_eq!(observations.control_plane_peer_id, None);
    assert!(observations
        .peers
        .iter()
        .any(|peer| peer.peer_id == a.sync.peer_id()));
    let anonymous = talos_service::Actor::regular(ACCOUNT);
    assert_eq!(
        a.service
            .sync_metrics(&anonymous)
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::Unauthenticated
    );
    let mismatch = talos_service::Actor::regular("other-account").with_access_token(TOKEN);
    assert_eq!(
        a.service
            .sync_metrics(&mismatch)
            .await
            .unwrap_err()
            .error_code(),
        ErrorCode::PermissionDenied
    );
    let other = talos_service::Actor::regular("other-account").with_access_token(OTHER_TOKEN);
    let isolated = a.service.sync_metrics(&other).await.unwrap();
    assert!(isolated.peers.is_empty());
    assert_eq!(isolated.metadata.computers, 0);
    assert_eq!(a.sync.saved_bindings().await.unwrap().len(), bindings);

    let rpc = TalosApplicationAdapter::new(a.service.clone(), 128);
    let request =
        talos_app_protocol::requests::ClientRequest::decode("sync/metrics", None).unwrap();
    let reply = rpc.dispatch(actor, request.clone()).await.unwrap();
    let rpc_metrics: talos_types::SyncMetricsResponse = serde_json::from_value(reply.body).unwrap();
    assert!(rpc_metrics
        .peers
        .iter()
        .any(|peer| peer.computer_ids.contains(a_id)));
    assert_eq!(
        rpc.dispatch(&anonymous, request).await.err().unwrap().code,
        ErrorCode::Unauthenticated
    );

    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let endpoint = format!("http://127.0.0.1:{gateway_http_port}/v1/sync/metrics");
    assert_eq!(client.get(&endpoint).send().await.unwrap().status(), 401);
    let response = client
        .get(&endpoint)
        .bearer_auth(TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["status"], "success");
    let http_metrics: talos_types::SyncMetricsResponse =
        serde_json::from_value(body["data"].clone()).unwrap();
    assert!(http_metrics.control_plane_peer_id.is_some());
    assert!(http_metrics
        .peers
        .iter()
        .any(|peer| peer.computer_ids.contains(b_id)));
    assert!(!body.to_string().contains(TOKEN));
    let other_body: Value = client
        .get(endpoint)
        .bearer_auth(OTHER_TOKEN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(other_body["data"]["peers"], json!([]));
}
