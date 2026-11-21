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

    use tokio::sync::Barrier;

    #[derive(Default)]
    struct GatewayAuth {
        calls: AtomicUsize,
        failure: Mutex<Option<TalosError>>,
        verification_pause: Mutex<Option<Arc<Barrier>>>,
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
            let pause = self.verification_pause.lock().unwrap().take();
            if let Some(pause) = pause {
                pause.wait().await;
                pause.wait().await;
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

    let verification_pause = Arc::new(Barrier::new(2));
    *gateway_auth.verification_pause.lock().unwrap() = Some(verification_pause.clone());
    let (result, pause) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(request(&owner, implicit.clone()), async {
            verification_pause.wait().await;
            let pause = restarted.sync_host.suspend_refresh(&owner).await;
            verification_pause.wait().await;
            pause
        })
    })
    .await
    .expect("the Gateway verification must overlap account suspension");
    assert_eq!(
        result.unwrap().unwrap_err().error_code(),
        ErrorCode::PermissionDenied
    );
    drop(pause);
    restarted.sync_host.resume_login(&owner);

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

#[tokio::test]
async fn cold_personal_chat_continues_on_its_server_with_session_commands() {
    use axum::{
        extract::{Request, State},
        Json, Router,
    };
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicBool, Ordering};
    use talos_app_protocol::requests::ClientRequest;
    use talos_app_server::AppServerApplication;
    use talos_types::{talos::SessionSource, ApiResponse, StartTurnRequest};

    const SESSION: &str = "sess_server_owned";
    const REPLACEMENT: &str = "sess_new_personal_chat";
    let replaced = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .fallback(
            |State(replaced): State<Arc<AtomicBool>>, request: Request| async move {
                let path = request.uri().path().to_owned();
                assert_eq!(request.headers()["authorization"], "Bearer owner");
                let data = if path == "/v1/computers/managed-worker" {
                    let session = if replaced.load(Ordering::Relaxed) {
                        REPLACEMENT
                    } else {
                        SESSION
                    };
                    json!({"enabled": true, "instance": {"instanceId":session, "nodeId":session,
                "conversationSessionId":session, "generation":1, "phase":"ready",
                "cpuCores":1, "memoryBytes":1024, "diskQuota":"unsupported"}})
                } else {
                    let body = axum::body::to_bytes(request.into_body(), 8192)
                        .await
                        .unwrap();
                    let params = serde_json::from_slice::<Value>(&body).unwrap();
                    json!({"path":path, "params":params})
                };
                Json(ApiResponse::success(data))
            },
        )
        .with_state(replaced.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let fixture = DesktopFixture::new().await;
    let mut host = fixture.host("Desktop");
    host.config.cloud.agent_base_url = origin;
    let actor = Actor::regular("owner")
        .with_user_type(UserType::Max)
        .with_access_token("owner");
    let turn: StartTurnRequest = serde_json::from_value(json!({
        "sessionId": SESSION, "input":{"content":[{"type":"text", "text":"Continue"}]}
    }))
    .unwrap();
    let response = host
        .start_turn(&actor, SessionSource::http(), turn.clone())
        .await
        .unwrap();
    assert_eq!(
        response.body,
        json!({"path":"/v1/turns/start", "params":turn})
    );

    for (method, path, params) in [
        (
            "session/compact",
            format!("/v1/sessions/{SESSION}/compact"),
            json!({"sessionId": SESSION}),
        ),
        (
            "session/clear",
            format!("/v1/sessions/{SESSION}/clear"),
            json!({"sessionId": SESSION}),
        ),
        (
            "automation/execute",
            "/v1/automation/execute".to_owned(),
            json!({
                "sessionId": SESSION, "action":{"target":"goal","action":{"type":"pause","goalId":"goal"}}
            }),
        ),
    ] {
        let request = ClientRequest::decode(method, Some(params.clone())).unwrap();
        let response = host.dispatch(&actor, request).await.unwrap();
        assert_eq!(response.body, json!({"path": path, "params":params}));
    }
    replaced.store(true, Ordering::Relaxed);
    let next_turn = StartTurnRequest {
        session_id: REPLACEMENT.into(),
        ..turn
    };
    let response = host
        .start_turn(&actor, SessionSource::http(), next_turn.clone())
        .await
        .unwrap();
    assert_eq!(
        response.body,
        json!({"path":"/v1/turns/start", "params":next_turn})
    );
    fixture.stop.cancel();
    fixture.server.await.unwrap().unwrap();
    server.abort();
}

#[tokio::test]
async fn offline_desktop_reads_an_ordinary_workers_stored_session() {
    use serde_json::json;
    use talos_app_protocol::requests::ClientRequest;
    use talos_app_server::AppServerApplication;

    let fixture = DesktopFixture::new().await;
    let mut record = session_record();
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
    host.config.cloud.agent_base_url = "http://127.0.0.1:0".into();

    let request =
        ClientRequest::decode("session/get", Some(json!({"sessionId":record.id}))).unwrap();
    let response = host.dispatch(&actor, request).await.unwrap();
    assert_eq!(response.body["session"]["nodeId"], record.execution_node_id);
    let history =
        ClientRequest::decode("session/history", Some(json!({"sessionId":record.id}))).unwrap();
    assert_eq!(
        host.dispatch(&actor, history).await.unwrap().body["data"],
        json!([])
    );

    let upload = ClientRequest::decode("file/upload", Some(json!({
        "request":{"filename":"offline.png","scope":{"type":"personal"},"tags":[],"metadata":{}},
        "dataBase64":"iVBORw0KGgo="
    }))).unwrap();
    let file = &host.dispatch(&actor, upload).await.unwrap().body["file"];
    let params = json!({"fileId":file["fileId"], "sessionId":record.id});
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
