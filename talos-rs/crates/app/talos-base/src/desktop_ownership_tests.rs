//! Cold local admission with document initialization and remote registration unavailable.
use super::*;

#[cfg(feature = "sync")]
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
    let owner = Actor::regular("user-1").with_access_token("user-1");
    fixture
        .runtime
        .host
        .refresh_computer_directory(&owner)
        .await
        .unwrap();
    let registered = host
        .enroll_worker(None, None)
        .await
        .unwrap()
        .node_id
        .unwrap();
    let view = fixture
        .runtime
        .host
        .computer_view(&owner, fixture.registered(&registered).await)
        .await
        .expect("first enrollment must be visible before the next directory refresh");
    assert_eq!(view.node_id, registered);
    host.stop_registration().await.unwrap();
    let identity = fixture.runtime.host.verified_actor(&owner).await.unwrap();
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
    fixture.runtime.shutdown_host().await.unwrap();
    fixture
        .runtime
        .worker_rpc
        .as_ref()
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    fixture.runtime.resource_stop.cancel();
    fixture.runtime.automation.shutdown().await.unwrap();
    fixture
        .runtime
        .worker
        .as_ref()
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    drop(host);
    drop(fixture.runtime);
    let restarted = crate::build_application_runtime_with_migration_dir(
        config,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
    )
    .await
    .unwrap();

    // Freeze all account maintenance so a directory pull cannot consume the
    // one-request authorization fault intended for the local access check.
    let maintenance = restarted.host.pause_refresh(&owner.user_id).await;
    let lifecycle = restarted.host.lifecycle.try_lock().unwrap();
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
    assert_eq!(
        status.local_computer_id.as_deref(),
        Some(registered.as_str())
    );
    assert_eq!(
        restarted.host.local_computer_id().await.unwrap(),
        registered
    );

    let implicit_models = request(&owner, implicit.clone()).await.unwrap().unwrap();
    let explicit_models = request(&owner, explicit.clone()).await.unwrap().unwrap();
    assert!(!implicit_models.models.is_empty());
    assert_eq!(implicit_models, explicit_models);
    let before = gateway_auth.calls.load(Ordering::Relaxed);
    for failure in [
        TalosError::new(
            ErrorCode::ServiceUnavailable,
            "Gateway temporarily unavailable",
        ),
        TalosError::localized(talos_error::ErrorLocalization::ComputerRegistrationRemoved),
    ] {
        *gateway_auth.failure.lock().unwrap() = Some(failure);
        assert_eq!(
            request(&owner, implicit.clone()).await.unwrap().unwrap(),
            implicit_models
        );
        *gateway_auth.failure.lock().unwrap() = None;
    }
    let missing_path = credential_path.with_extension("held");
    std::fs::rename(&credential_path, &missing_path).unwrap();
    let local_models = request(&owner, explicit.clone()).await.unwrap().unwrap();
    std::fs::rename(&missing_path, &credential_path).unwrap();
    assert_eq!(local_models, implicit_models);
    assert_eq!(
        gateway_auth.calls.load(Ordering::Relaxed),
        before,
        "Local node reads must not require remote registration verification"
    );

    // Token validation belongs to the entrypoint, before a trusted Service Actor exists.
    let error = restarted
        .auth_service
        .verify_access_token("expired")
        .await
        .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::Unauthenticated);
    assert!(restarted.auth_service.local_identity("expired").is_err());
    let foreign_identity = restarted
        .auth_service
        .verify_access_token("user-2")
        .await
        .unwrap();
    let foreign = Actor::regular(foreign_identity.user_id)
        .with_role(foreign_identity.role)
        .with_user_type(foreign_identity.user_type)
        .with_access_token(foreign_identity.access_token.unwrap());
    for (actor, selector, code) in [
        (&foreign, implicit.clone(), ErrorCode::PermissionDenied),
        (&foreign, explicit.clone(), ErrorCode::ResourceNotFound),
    ] {
        let error = request(actor, selector).await.unwrap().unwrap_err();
        assert_eq!(error.error_code(), code);
    }

    assert!(restarted.worker_registration.lock().await.is_none());
    let pause = restarted.host.suspend_refresh(&owner).await;
    assert!(!restarted.host.account_connections_allowed(&owner.user_id));
    // Logout invalidates credential admission; background suspension is not a
    // second authorization gate inside an already authenticated local Service call.
    restarted.auth_service.invalidate_access_token("user-1");
    assert_eq!(
        restarted
            .auth_service
            .local_identity("user-1")
            .unwrap_err()
            .error_code(),
        ErrorCode::Unauthenticated
    );
    restarted
        .auth_service
        .verify_access_token("user-1")
        .await
        .unwrap();
    drop(pause);
    restarted.host.resume_login(&owner);

    // Rotating remote publication credentials does not revoke local product access.
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
        assert_eq!(
            request(&owner, selector).await.unwrap().unwrap(),
            implicit_models
        );
    }
    assert_eq!(std::fs::read(&credential_path).unwrap(), credential);
    let runner = restarted.config.agent_node.runner.as_ref().unwrap();
    YamlConfigStore::new(restarted.config.source_path())
        .save_node_identity(runner, &replacement.token)
        .unwrap();

    // Hold publication of the saved document handle. Worker registration must
    // complete before document initialization can even reach its bad CA.
    let documents = restarted.host.stores.handles.lock().await;
    drop(maintenance);
    drop(lifecycle);
    restarted.service.resume_account_host(&owner);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if restarted.host.local_owner.read().unwrap().as_deref() == Some("user-1") {
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

    restarted.shutdown_host().await.unwrap();
    if let Some(worker) = restarted.worker_registration.lock().await.take() {
        worker.shutdown().await.unwrap();
    }
    restarted
        .worker_rpc
        .as_ref()
        .unwrap()
        .shutdown()
        .await
        .unwrap();
    restarted.resource_stop.cancel();
    restarted.automation.shutdown().await.unwrap();
    restarted.worker.as_ref().unwrap().shutdown().await.unwrap();
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
    cloud.abort();
}

#[tokio::test]
async fn offline_desktop_reads_its_own_session_without_falling_back_for_remote_products() {
    use serde_json::json;
    use talos_app_protocol::requests::ClientRequest;
    use talos_app_server::AppServerApplication;

    let fixture = DesktopFixture::new().await;
    let mut record = session_record();
    record.execution_node_id = "embedded".into();
    record.agent_id = fixture.runtime.worker.as_ref().unwrap().deployments()[0]
        .agent_id
        .clone();
    record.conf.model_selection =
        Some(talos_types::resolve_default_model_selection(&fixture.config.llm).unwrap());
    fixture
        .runtime
        .session_store
        .insert_session_record(&record)
        .await
        .unwrap();
    let actor = Actor::regular(&record.user_id)
        .with_user_type(UserType::Max)
        .with_access_token("offline");
    let mut host = fixture.host("Offline Desktop");
    host.config.cloud.agent_base_url = "file:///unavailable-control-plane".into();

    let request =
        ClientRequest::decode("session/get", Some(json!({"sessionId":record.id}))).unwrap();
    let response = host.dispatch(&actor, request).await.unwrap();
    assert_eq!(response.body["session"]["nodeId"], record.execution_node_id);
    let mut remote =
        ClientRequest::decode("session/get", Some(json!({"sessionId":record.id}))).unwrap();
    remote.computer_id = Some("remote-computer".into());
    assert!(
        host.dispatch(&actor, remote).await.is_err(),
        "Unavailable remote ownership must not read a matching local Session"
    );
    let history =
        ClientRequest::decode("session/history", Some(json!({"sessionId":record.id}))).unwrap();
    assert_eq!(
        host.dispatch(&actor, history).await.unwrap().body["data"],
        json!([])
    );

    let recovery =
        ClientRequest::decode("timeline/recover", Some(json!({"sessionId":record.id}))).unwrap();
    let replay = host.dispatch(&actor, recovery).await.unwrap().body;
    assert_eq!(replay["replay"]["events"], json!([]));

    let mut subscription = host
        .subscribe_timeline_events(
            &actor,
            serde_json::from_value(json!({
                "sessionId":record.id, "closeWhenIdle":true
            }))
            .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        matches!(subscription.events.recv().await, Some(talos_service::TimelineSubscriptionMessage::Closed(closed)) if closed.reason == talos_service::TimelineSubscriptionCloseReason::Completed)
    );

    let upload = ClientRequest::decode("file/upload", Some(json!({
        "request":{"filename":"offline.png","scope":{"type":"personal"},"tags":[],"metadata":{}},
        "dataBase64":"iVBORw0KGgo="
    }))).unwrap();
    let file = &host.dispatch(&actor, upload).await.unwrap().body["file"];
    let list = ClientRequest::decode("file/list", Some(json!({}))).unwrap();
    assert_eq!(
        host.dispatch(&actor, list).await.unwrap().body["data"],
        json!([file])
    );
    let params = json!({"fileId":file["fileId"]});
    let read = ClientRequest::decode("file/read", Some(params.clone())).unwrap();
    assert_eq!(
        host.dispatch(&actor, read).await.unwrap().body["file"],
        *file
    );
    let content = ClientRequest::decode("file/content", Some(params)).unwrap();
    assert_eq!(
        host.dispatch(&actor, content).await.unwrap().body["dataBase64"],
        "iVBORw0KGgo="
    );

    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}

#[tokio::test]
async fn stdio_reports_the_attached_runtime_managed_worker_capability() {
    use serde_json::json;
    use talos_app_protocol::requests::ClientRequest;
    use talos_app_server::AppServerApplication;

    let fixture = DesktopFixture::new().await;
    let mut host = fixture.host("Desktop");
    host.config.cloud.agent_base_url = "file:///unavailable-control-plane".into();
    let actor = Actor::regular("owner")
        .with_user_type(UserType::Max)
        .with_access_token("owner");
    let status = ClientRequest::decode("computer/readManagedWorker", None).unwrap();
    assert_eq!(
        host.dispatch(&actor, status).await.unwrap().body,
        json!({"enabled": false, "instance": null})
    );
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
}
