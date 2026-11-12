//! Cold local admission with document initialization and remote registration unavailable.
use super::*;

#[tokio::test]
async fn cached_local_requests_check_ownership_independently_of_document_sync() {
    use axum::{routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
        time::Duration,
    };

    #[derive(Default)]
    struct GatewayAuth {
        calls: AtomicUsize,
        failure: Mutex<Option<TalosError>>,
    }

    #[tonic::async_trait]
    impl AuthService for GatewayAuth {
        async fn logout(&self, identity: &RequestIdentity) -> TalosResult<()> {
            TestAuth.logout(identity).await
        }

        async fn verify_access_token(&self, token: &str) -> TalosResult<RequestIdentity> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let failure = self.failure.lock().unwrap().take();
            if let Some(error) = failure {
                return Err(error);
            }
            TestAuth.verify_access_token(token).await
        }
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let api = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().route(
        "/v1/auth/introspect",
        post(|Json(request): Json<Value>| async move {
            let token = request["token"].as_str().unwrap();
            Json(json!({
                "active": matches!(token, "user-1" | "user-2"),
                "user": {"user_id": token, "email": "test@example.invalid", "username": token, "role": "user", "type": "free", "status": "active"},
                "capabilities": {"is_admin": false, "role": "user", "scopes": []},
                "entitlement": {"can_use": true, "plan": "free", "reason": null},
                "expires_at_ms": null,
            }))
        }),
    );
    let cloud = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let gateway_auth = Arc::new(GatewayAuth::default());
    let fixture = DesktopFixture::with_auth(gateway_auth.clone(), Some(api)).await;
    let host = fixture.host("Saved Computer");
    fixture.login("user-1");
    let registered = host
        .enroll_worker(None, None)
        .await
        .unwrap()
        .node_id
        .unwrap();
    host.stop_registration().await.unwrap();
    let owner = Actor::regular("user-1").with_access_token("user-1");
    let identity = fixture
        .runtime
        .sync_host
        .verified_actor(&owner)
        .await
        .unwrap();
    fixture
        .runtime
        .sync
        .bind_account(
            &identity,
            talos_sync::SyncBindParams {
                scope: talos_sync::SyncScope {
                    hub_id: "unavailable-documents".into(),
                    account_id: owner.user_id.clone(),
                },
                source_user_id: owner.user_id.clone(),
                computers: vec![],
            },
        )
        .await
        .unwrap();
    let mut config = fixture.config.reload().unwrap();
    config.sync.enabled = Some(true);
    config.sync.trusted_ca_file = Some(fixture.home.path().join("missing-sync-ca.pem"));
    let credential_path = config
        .agent_node
        .runner
        .as_ref()
        .unwrap()
        .credential_file
        .clone();
    let credential = std::fs::read(&credential_path).unwrap();
    persist_desktop_config(&config);
    let restarted = crate::build_application_runtime_with_migration_dir(
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();

    let lifecycle = restarted.sync_host.lifecycle.try_lock().unwrap();
    let registration = restarted.worker_registration.lock().await.take().unwrap();
    registration.shutdown().await.unwrap();
    let implicit = talos_types::ModelListRequest::default();
    let explicit = talos_types::ModelListRequest {
        node_id: Some(registered.clone()),
    };
    let request = |actor, selector| {
        tokio::time::timeout(
            Duration::from_secs(3),
            restarted.service.list_models(actor, selector),
        )
    };
    let status = restarted.service.sync_status(&owner).await.unwrap();
    assert_eq!(status.local_computer_id, None);
    assert_eq!(
        restarted.sync_host.local_computer_id().await.unwrap(),
        registered
    );

    let transient = TalosError::new(
        ErrorCode::ServiceUnavailable,
        "Gateway temporarily unavailable",
    );
    let removed =
        TalosError::localized(talos_error::ErrorLocalization::ComputerRegistrationRemoved);
    for failure in [transient, removed] {
        let expected = failure.to_api_error();
        *gateway_auth.failure.lock().unwrap() = Some(failure);
        let received = request(&owner, implicit.clone())
            .await
            .expect("return the check failure while the supervisor is blocked")
            .unwrap_err();
        assert_eq!(received.to_api_error(), expected);
    }

    let missing_path = credential_path.with_extension("held");
    std::fs::rename(&credential_path, &missing_path).unwrap();
    let file_error = request(&owner, explicit.clone())
        .await
        .unwrap()
        .unwrap_err();
    std::fs::rename(&missing_path, &credential_path).unwrap();
    assert!(file_error.is_code(ErrorCode::InternalError));
    assert!(std::error::Error::source(&file_error).is_some());
    assert!(file_error.location().file().ends_with("standalone.rs"));

    let invalid = Actor::regular("user-1").with_access_token("expired");
    let foreign = Actor::regular("user-2").with_access_token("user-2");
    for (actor, selector, code) in [
        (&invalid, implicit.clone(), ErrorCode::Unauthenticated),
        (&foreign, implicit.clone(), ErrorCode::PermissionDenied),
        (&foreign, explicit.clone(), ErrorCode::ResourceNotFound),
    ] {
        let error = request(actor, selector).await.unwrap().unwrap_err();
        assert_eq!(error.error_code(), code);
    }

    let before = gateway_auth.calls.load(Ordering::Relaxed);
    let implicit_models = request(&owner, implicit.clone()).await.unwrap().unwrap();
    let explicit_models = request(&owner, explicit.clone()).await.unwrap().unwrap();
    assert!(!implicit_models.models.is_empty());
    assert_eq!(implicit_models, explicit_models);
    assert_eq!(
        gateway_auth.calls.load(Ordering::Relaxed) - before,
        2,
        "each local request needs only one Gateway verification"
    );
    assert!(restarted.worker_registration.lock().await.is_none());

    // A control-plane rotation leaves the saved bytes intact but revokes their proof.
    let replacement = fixture
        .nodes
        .rotate_credential(
            &owner,
            talos_types::RotateCredentialRequest {
                node_id: registered.clone(),
                expires_at: CloudSessionCache::now_ms() / 1000 + 600,
            },
        )
        .await
        .unwrap();
    for selector in [implicit.clone(), explicit] {
        let error = request(&owner, selector).await.unwrap().unwrap_err();
        assert_eq!(error.error_code(), ErrorCode::ResourceExpired);
        assert!(error
            .to_api_error()
            .message
            .contains("Recover this computer credential"));
    }
    assert_eq!(std::fs::read(&credential_path).unwrap(), credential);
    let runner = restarted.config.agent_node.runner.as_ref().unwrap();
    YamlConfigStore::new(restarted.config.source_path())
        .save_node_identity(runner, &replacement.token)
        .unwrap();

    // Hold publication of the saved document handle. Worker registration must
    // complete before document initialization can even reach its bad CA.
    let documents = restarted.sync_host.sync_stores.handles.lock().await;
    drop(lifecycle);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if restarted.sync_host.local_owner.read().unwrap().as_deref() == Some("user-1") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("saved Worker ownership must settle while document publication is blocked");
    assert!(documents.is_empty());
    drop(documents);

    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let status = restarted.service.sync_status(&owner).await.unwrap();
            if status.last_error.is_some_and(|error| {
                error.code == ErrorCode::InternalError
                    && error.message.starts_with("Read sync trust certificate")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("document initialization must report its original error after publication is released");
    assert_eq!(
        request(&owner, implicit).await.unwrap().unwrap(),
        implicit_models,
        "a failed document association must not stop local execution"
    );
    assert!(restarted.worker_registration.lock().await.is_some());

    restarted.shutdown_sync().await.unwrap();
    if let Some(worker) = restarted.worker_registration.lock().await.take() {
        worker.shutdown().await.unwrap();
    }
    restarted.worker_rpc.shutdown().await.unwrap();
    restarted.resource_rpc.shutdown().await.unwrap();
    restarted.automation.shutdown().await.unwrap();
    restarted.worker.shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
    cloud.abort();
}
