use super::credentials::actor_credentials_for_method;
use super::*;
use bytes::Bytes;
use std::collections::BTreeMap;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentTurnRef, AgentTurnStarted, TimelineEvent,
};
use talos_app_protocol::jsonrpc::{JsonRpcRequest, RequestId};

#[test]
fn skips_cached_actor_credentials_for_local_session_methods() {
    let credentials = Some(ActorCredentials {
        credential: ActorCredential::ExistingSessionToken {
            token: "cloud-token".to_string(),
        },
    });

    assert_eq!(
        actor_credentials_for_method("initialize", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/session/read", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/session/clear", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/start", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/status", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("auth/login/cancel", &credentials),
        None
    );
    assert_eq!(
        actor_credentials_for_method("session/list", &credentials),
        credentials
    );
}

#[test]
fn touches_cached_session_only_for_business_methods() {
    assert!(!is_business_method("initialize"));
    assert!(!is_business_method("auth/session/read"));
    assert!(!is_business_method("auth/session/clear"));
    assert!(!is_business_method("auth/login/start"));
    assert!(!is_business_method("auth/login/status"));
    assert!(!is_business_method("auth/login/cancel"));
    assert!(is_business_method("user/current"));
    assert!(is_business_method("profile/read"));
    assert!(is_business_method("session/list"));
}

#[test]
fn file_upload_params_use_canonical_padded_base64() {
    let params = files::file_upload_params(
        talos_types::FileUploadRequest {
            filename: "note.txt".to_string(),
            scope: talos_types::FileScope::Personal,
            tags: Vec::new(),
            metadata: BTreeMap::new(),
        },
        Bytes::from_static(b"ab"),
    );

    assert_eq!(params.data_base64, "YWI=");
    assert_eq!(params.request.filename, "note.txt");
}

#[test]
fn decodes_message_queue_change_notification_without_timeline_event() {
    let notification = JsonRpcNotification::new(
        "messageQueue/changed",
        Some(serde_json::json!({ "sessionId": "session-1", "revision": 6 })),
    );

    assert_eq!(
        response::decode_message_queue_changed(&notification).expect("notification should decode"),
        Some(talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 6,
        })
    );
}

#[test]
fn decodes_slash_timeline_notification() {
    let expected = talos_app_protocol::events::TimelineEventNotification {
        subscription_id: "sub-1".to_string(),
        event: timeline_event("ev_1", 1),
    };
    let notification = JsonRpcNotification::new(
        "timeline/turn/started",
        Some(serde_json::to_value(&expected).expect("notification should serialize")),
    );

    let decoded = response::decode_event_notification(notification)
        .expect("notification should decode")
        .expect("timeline method should be recognized");

    assert_eq!(decoded.subscription_id, expected.subscription_id);
    assert_eq!(decoded.event.event_id, expected.event.event_id);
    assert_eq!(decoded.event.event, expected.event.event);
}

#[tokio::test]
async fn rejected_cached_credentials_preserve_the_original_request_error() {
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("start local pipe fixture");
    let mut client = test_app_server_client(/*use_cloud_session_cache*/ true);
    client.stdin = child.stdin.take();
    client.actor_credentials = Some(existing_session_credentials("expired-token".to_string()));
    let expected = TalosError::from_source(
        talos_error::ErrorCode::Unauthenticated,
        "The login session has expired.",
        std::io::Error::other("private authentication diagnostic"),
    );
    let pending = client.pending_responses.clone();
    let mut lines = BufReader::new(child.stdout.take().expect("fixture stdout")).lines();
    let response_error = expected.clone();
    let respond = async move {
        let line = lines
            .next_line()
            .await
            .expect("read request")
            .expect("request line");
        let request: talos_app_protocol::jsonrpc::JsonRpcRequest =
            serde_json::from_str(&line).expect("request frame");
        let talos_app_protocol::jsonrpc::RequestId::Number(id) = request.id else {
            panic!("numeric SDK request ID");
        };
        pending
            .lock()
            .await
            .remove(&id)
            .expect("pending request")
            .send(Err(response_error))
            .expect("request receiver");
    };
    let (result, ()) = tokio::join!(client.request_value("session/list", ()), respond);
    let error = result.expect_err("expired login must fail");
    assert_eq!(error.to_api_error(), expected.to_api_error());
    assert!(std::ptr::eq(error.backtrace(), expected.backtrace()));
    assert!(client.actor_credentials.is_none());
    drop(client.stdin.take());
    child.wait().await.expect("fixture exit");
}

#[tokio::test]
async fn cold_cached_session_verifies_token_before_sync_status() {
    let mut child = Command::new("cat")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut client = test_app_server_client(true);
    client.stdin = child.stdin.take();
    let pending = client.pending_responses.clone();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let token = "cached-token";
    let cache = AuthSessionReadResponse {
        session: Some(talos_types::AuthSessionSnapshot {
            schema_version: 1,
            cloud_api_base_url: "https://cloud.example".into(),
            access_token: token.into(),
            token_type: "Bearer".into(),
            expires_at_ms: None,
            last_used_at_ms: "0".into(),
            user: talos_types::AuthSessionUserSnapshot {
                user_id: "user-1".into(),
                username: "ada".into(),
                email: None,
                role: talos_auth::UserRole::User,
                r#type: talos_auth::UserType::Pro,
            },
        }),
    };
    let rejected = TalosError::new(
        talos_error::ErrorCode::Unauthenticated,
        "The login session has expired.",
    );
    let respond = async {
        for (method, credentials, response) in [
            (
                "auth/session/read",
                None,
                Ok(serde_json::to_value(cache).unwrap()),
            ),
            (
                "user/current",
                Some(existing_session_credentials(token.into())),
                Err(rejected.clone()),
            ),
        ] {
            let request: JsonRpcRequest =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request.method, method);
            assert_eq!(request.actor_credentials, credentials);
            let RequestId::Number(id) = request.id else {
                panic!("numeric SDK request ID");
            };
            pending
                .lock()
                .await
                .remove(&id)
                .unwrap()
                .send(response)
                .unwrap();
        }
    };
    let (result, ()) = tokio::join!(client.restore_cloud_session_candidate(), respond);
    assert_eq!(result.unwrap_err().to_api_error(), rejected.to_api_error());
    assert!(client.actor_credentials.is_none());
    drop(client.stdin.take());
    child.wait().await.unwrap();
}

#[tokio::test]
async fn slow_plugin_requests_accept_delayed_responses_without_extending_other_requests() {
    for method in ["plugin/install", "plugin/update", "session/list"] {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut client = test_app_server_client(false);
        client.stdin = child.stdin.take();
        let pending = client.pending_responses.clone();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        {
            let mut request = std::pin::pin!(client.request_value(method, ()));
            let line = tokio::select! {
                result = &mut request => panic!("request completed before its response: {result:?}"),
                line = lines.next_line() => line.unwrap().unwrap(),
            };
            let frame: JsonRpcRequest = serde_json::from_str(&line).unwrap();
            let RequestId::Number(id) = frame.id else {
                panic!("numeric request ID")
            };
            tokio::time::pause();
            tokio::time::advance(Duration::from_secs(31)).await;
            let state = std::future::poll_fn(|context| {
                std::task::Poll::Ready(std::future::Future::poll(request.as_mut(), context))
            })
            .await;
            if method == "session/list" {
                assert!(matches!(state, std::task::Poll::Ready(Err(_))));
                assert!(pending.lock().await.is_empty());
            } else {
                assert!(
                    state.is_pending(),
                    "Plugin preparation may exceed 30 seconds"
                );
                let expected = serde_json::json!({"accepted": true});
                pending
                    .lock()
                    .await
                    .remove(&id)
                    .unwrap()
                    .send(Ok(expected.clone()))
                    .unwrap();
                assert_eq!(request.await.unwrap(), expected);
            }
            tokio::time::resume();
        }
        drop(client.stdin.take());
        child.wait().await.unwrap();
    }
}

#[tokio::test]
async fn cloud_login_initializes_owned_host_with_the_returned_token() {
    for use_cache in [false, true] {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let mut client = test_app_server_client(use_cache);
        client.stdin = child.stdin.take();
        let pending = client.pending_responses.clone();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let response = authenticated_login_response("issued-login-token");
        let respond = async move {
            let request: talos_app_protocol::jsonrpc::JsonRpcRequest =
                serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request.method, "sync/status");
            assert_eq!(
                request.actor_credentials,
                Some(existing_session_credentials("issued-login-token".into()))
            );
            let talos_app_protocol::jsonrpc::RequestId::Number(id) = request.id else {
                panic!("numeric SDK request ID");
            };
            let status = talos_types::SyncStatusResponse {
                peer_id: None,
                configured_enabled: Some(false),
                connection: talos_types::SyncConnectionState::Disabled,
                pending_changes: 0,
                last_acknowledged_at: None,
                last_error: None,
                local_computer_id: Some("owned-computer".into()),
                can_claim_local_history: true,
            };
            pending
                .lock()
                .await
                .remove(&id)
                .unwrap()
                .send(Ok(serde_json::to_value(status).unwrap()))
                .unwrap();
        };
        let (result, ()) = tokio::join!(client.apply_cloud_login_credentials(&response), respond);
        result.unwrap();
        assert_eq!(
            client.actor_credentials,
            Some(existing_session_credentials("issued-login-token".into()))
        );
        drop(client.stdin.take());
        child.wait().await.unwrap();
    }
}

#[tokio::test]
async fn dispatch_timeline_event_routes_only_matching_subscription() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (second_tx, mut second_rx) = mpsc::channel(4);
    let (first_queue_tx, _first_queue_rx) = mpsc::channel(4);
    let (second_queue_tx, _second_queue_rx) = mpsc::channel(4);
    {
        let mut streams = subscription_senders.lock().await;
        streams.insert(
            "sub-1".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: first_tx,
                automation_sender: mpsc::channel(4).0,
                message_queue_sender: first_queue_tx,
            },
        );
        streams.insert(
            "sub-2".to_string(),
            SubscriptionStream {
                session_id: "session-1".to_string(),
                sender: second_tx,
                automation_sender: mpsc::channel(4).0,
                message_queue_sender: second_queue_tx,
            },
        );
    }

    dispatch_timeline_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_event("ev_1", 1),
    )
    .await;

    assert_eq!(first_rx.recv().await.expect("first event").event_id, "ev_1");
    assert!(second_rx.try_recv().is_err());
}

#[tokio::test]
async fn dispatch_timeline_event_buffers_until_stream_registration() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let pending_subscription_events = Arc::new(Mutex::new(HashMap::new()));

    dispatch_timeline_event(
        &subscription_senders,
        &pending_subscription_events,
        "sub-1",
        timeline_event("ev_1", 1),
    )
    .await;

    let buffered = pending_subscription_events
        .lock()
        .await
        .remove("sub-1")
        .expect("buffered event");
    assert_eq!(buffered.len(), 1);
    assert_eq!(buffered[0].event_id, "ev_1");
}

#[tokio::test]
async fn message_queue_change_closes_a_lagged_sdk_subscription() {
    let subscription_senders = Arc::new(Mutex::new(HashMap::new()));
    let (timeline_tx, mut timeline_rx) = mpsc::channel(1);
    let (queue_tx, _queue_rx) = mpsc::channel(1);
    queue_tx
        .try_send(talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 1,
        })
        .expect("fill queue channel");
    subscription_senders.lock().await.insert(
        "sub-1".to_string(),
        SubscriptionStream {
            session_id: "session-1".to_string(),
            sender: timeline_tx,
            automation_sender: mpsc::channel(4).0,
            message_queue_sender: queue_tx,
        },
    );

    dispatch_message_queue_change(
        &subscription_senders,
        talos_types::MessageQueueChangedNotification {
            session_id: "session-1".to_string(),
            revision: 2,
        },
    )
    .await;

    assert!(subscription_senders.lock().await.is_empty());
    assert!(timeline_rx.recv().await.is_none());
}

fn timeline_event(event_id: &str, event_seq: i64) -> TimelineEvent {
    TimelineEvent {
        event_id: event_id.to_string(),
        event_seq,
        persisted_at: event_seq,
        event: talos_agent_api::TimelineEventPayload::Agent(AgentEvent {
            identity: AgentTurnRef::new("session-1", "turn-1"),
            sequence: event_seq as u64,
            occurred_at: event_seq,
            payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        }),
    }
}

fn test_app_server_client(use_cloud_session_cache: bool) -> AppServerClient {
    let (_events_tx, events) = mpsc::channel(1);
    let (_notifications_tx, notifications) = mpsc::channel(1);
    AppServerClient {
        product_http: None,
        computer_id: None,
        ownership: AppServerOwnership::BorrowedProcess,
        child: None,
        stdin: None,
        next_id: 1,
        pending_responses: Arc::new(Mutex::new(HashMap::new())),
        subscription_senders: Arc::new(Mutex::new(HashMap::new())),
        pending_subscription_events: Arc::new(Mutex::new(HashMap::new())),
        events,
        notifications,
        actor_credentials: None,
        use_cloud_session_cache,
    }
}

fn authenticated_login_response(access_token: &str) -> CloudLoginStatusResponse {
    CloudLoginStatusResponse::Authenticated {
        user: talos_auth::UserDto {
            user_id: "user-1".to_string(),
            username: "ada".to_string(),
            email: None,
            role: talos_auth::UserRole::User,
            r#type: talos_auth::UserType::Pro,
            disabled: false,
            created_at: 0,
            updated_at: 0,
        },
        token_type: "Bearer".to_string(),
        access_token: access_token.to_string(),
    }
}

#[test]
fn decodes_automation_invalidation_separately_from_timeline() {
    let notification = talos_app_protocol::JsonRpcNotification::new(
        "automation/changed",
        Some(serde_json::json!({"sessionId":"session-1"})),
    );
    let change = response::decode_automation_changed(&notification)
        .unwrap()
        .unwrap();
    assert_eq!(change.session_id, "session-1");
    assert!(response::decode_message_queue_changed(&notification)
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn automation_changes_are_scoped_and_close_lagged_subscription() {
    let senders = Arc::new(Mutex::new(HashMap::new()));
    let (timeline_tx, _timeline_rx) = mpsc::channel(1);
    let (queue_tx, _queue_rx) = mpsc::channel(1);
    let (automation_tx, mut automation_rx) = mpsc::channel(1);
    senders.lock().await.insert(
        "sub-1".into(),
        SubscriptionStream {
            session_id: "session-1".into(),
            sender: timeline_tx,
            message_queue_sender: queue_tx,
            automation_sender: automation_tx,
        },
    );
    dispatch_automation_change(
        &senders,
        talos_types::AutomationChangedNotification {
            session_id: "other".into(),
        },
    )
    .await;
    assert!(automation_rx.try_recv().is_err());
    dispatch_automation_change(
        &senders,
        talos_types::AutomationChangedNotification {
            session_id: "session-1".into(),
        },
    )
    .await;
    assert_eq!(automation_rx.recv().await.unwrap().session_id, "session-1");
    for _ in 0..2 {
        dispatch_automation_change(
            &senders,
            talos_types::AutomationChangedNotification {
                session_id: "session-1".into(),
            },
        )
        .await;
    }
    assert!(senders.lock().await.is_empty());
}

#[tokio::test]
async fn product_http_preserves_typed_request_and_authenticated_computer_scope() {
    use axum::{routing::post, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/v1/app/request",
        post(
            |headers: axum::http::HeaderMap,
             Json(params): Json<talos_app_protocol::requests::ClientRequestParams>| async move {
                assert_eq!(headers["authorization"], "Bearer private-token");
                assert_eq!(headers["x-talos-computer"], "computer-a");
                assert!(matches!(
                    params,
                    talos_app_protocol::requests::ClientRequestParams::ModelList(_)
                ));
                Json(serde_json::json!({"status":"success","data":{"source":"cloud-product"}}))
            },
        ),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut client = AppServerClient::connect_product_http(
        &endpoint,
        "private-token".into(),
        "computer-a".into(),
        None,
    )
    .unwrap();
    let value = client
        .request_value("model/list", ModelListRequest { node_id: None })
        .await
        .unwrap();
    assert_eq!(value["source"], "cloud-product");
    task.abort();
}

#[tokio::test]
async fn product_http_streams_timeline_without_importing_remote_session_state() {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let expected = timeline_event("event-remote", 1);
    let body = format!(
        "event: timeline/agent\ndata: {}\n\nevent: subscription/terminated\ndata: {{}}\n\n",
        serde_json::to_string(&expected).unwrap()
    );
    let router = Router::new().route(
        "/v1/sessions/session-1/events",
        get(move |headers: axum::http::HeaderMap| {
            let body = body.clone();
            async move {
                assert_eq!(headers["x-talos-computer"], "computer-a");
                ([("content-type", "text/event-stream")], body)
            }
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut client =
        AppServerClient::connect_product_http(&endpoint, "token".into(), "computer-a".into(), None)
            .unwrap();
    let mut stream = client
        .open_timeline_stream(SubscribeTimelineRequest {
            session_id: "session-1".into(),
            turn_id: None,
            since_event_id: None,
            replay_limit: Some(10),
            close_when_idle: Some(false),
            close_on_terminal: None,
        })
        .await
        .unwrap();
    assert_eq!(stream.stream.recv().await.unwrap(), expected);
    client.shutdown().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn desktop_grant_uses_central_media_api_with_the_selected_computer() {
    use axum::{routing::post, Json, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new().route(
        "/v1/desktop/open",
        post(
            |headers: axum::http::HeaderMap,
             Json(params): Json<talos_types::DesktopOpenRequest>| async move {
                assert_eq!(headers["authorization"], "Bearer private-token");
                assert_eq!(params.node_id, "personal-computer");
                Json(serde_json::json!({"status":"success","data":{"ticket":"media-grant"}}))
            },
        ),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut client = AppServerClient::connect_product_http(
        &endpoint,
        "private-token".into(),
        "personal-computer".into(),
        None,
    )
    .unwrap();
    let response = client
        .request_value(
            "desktop/open",
            talos_types::DesktopOpenRequest {
                node_id: "embedded".into(),
                target_id: "managed".into(),
                viewer_id: "viewer".into(),
                generation: "generation".into(),
                input_mode: talos_types::BrowserInputMode::ReadOnly,
                controller_id: None,
                input_epoch: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(response["ticket"], "media-grant");
    task.abort();
}

struct BootstrapGate {
    ready: std::sync::atomic::AtomicBool,
    attempts: std::sync::atomic::AtomicUsize,
    bootstraps: std::sync::atomic::AtomicUsize,
    source_peer: bool,
    reject_bootstrap: bool,
    reject_retry: bool,
}

async fn fresh_product_server(
    source_peer: bool,
    reject_bootstrap: bool,
    reject_retry: bool,
) -> (String, Arc<BootstrapGate>, tokio::task::JoinHandle<()>) {
    use axum::{response::IntoResponse, Json, Router};
    use std::sync::atomic::Ordering;
    let gate = Arc::new(BootstrapGate {
        ready: false.into(),
        attempts: 0.into(),
        bootstraps: 0.into(),
        source_peer,
        reject_bootstrap,
        reject_retry,
    });
    let state = gate.clone();
    let router = Router::new().fallback(move |method: axum::http::Method, uri: axum::http::Uri,
        headers: axum::http::HeaderMap, body: Bytes| {
        let state = state.clone();
        async move {
            assert_eq!(headers["authorization"], "Bearer private-token");
            let denied = || (axum::http::StatusCode::UNAUTHORIZED, Json(talos_types::ApiResponse::<Value>::error(
                talos_error::ApiError::new(talos_error::ErrorCode::Unauthenticated)))).into_response();
            if uri.path() == "/v1/user" {
                assert_eq!(method, axum::http::Method::GET);
                assert!(!headers.contains_key("x-talos-computer"));
                assert!(!headers.contains_key("x-talos-source-peer"));
                state.bootstraps.fetch_add(1, Ordering::SeqCst);
                if state.reject_bootstrap { return denied(); }
                state.ready.store(true, Ordering::SeqCst);
                return Json(talos_types::ApiResponse::success(talos_types::UserCurrentResponse {
                    user: talos_auth::UserDto { user_id: "account".into(), username: "account".into(), email: None,
                        role: Default::default(), r#type: Default::default(), disabled: false, created_at: 0, updated_at: 0 },
                })).into_response();
            }
            state.attempts.fetch_add(1, Ordering::SeqCst);
            if state.source_peer {
                assert_eq!(headers["x-talos-source-peer"], "source-a");
                assert!(!headers.contains_key("x-talos-computer"));
            } else {
                assert_eq!(headers["x-talos-computer"], "computer-a");
            }
            if !state.ready.load(Ordering::SeqCst) || state.reject_retry { return denied(); }
            match uri.path() {
                "/v1/app/request" => {
                    let params: talos_app_protocol::requests::ClientRequestParams = serde_json::from_slice(&body).unwrap();
                    assert!(matches!(params, talos_app_protocol::requests::ClientRequestParams::ModelList(ref request) if request.node_id.as_deref() == Some("computer-a")));
                    Json(serde_json::json!({"status":"success","data":{"source":"fresh-server"}})).into_response()
                }
                "/v1/sessions/session-1/events" => {
                    assert!(uri.query().unwrap().contains("limit=10"));
                    let event = timeline_event("fresh-server-event", 1);
                    ([("content-type", "text/event-stream")], format!("event: timeline/agent\ndata: {}\n\nevent: subscription/terminated\ndata: {{}}\n\n",serde_json::to_string(&event).unwrap())).into_response()
                }
                "/v1/files/file-a/content" => {
                    assert_eq!(uri.query(), Some("offset=2&limit=3"));
                    Json(talos_types::ApiResponse::success(talos_types::FileContentResponse {
                        file: talos_types::FileInfo {file_id: "file-a".into(), scope: talos_types::FileScope::Personal,
                            original_filename: "a.txt".into(), mime_type: "text/plain".into(), size_bytes: 8,
                            tags: vec![], metadata: Default::default(), created_at: 0, updated_at: 0},
                        data_base64: "Y2Rl".into(),
                    })).into_response()
                }
                _ => panic!("unexpected product API {}", uri.path()),
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (endpoint, gate, task)
}

#[tokio::test]
async fn fresh_product_request_bootstraps_central_account_then_replays_the_same_scope() {
    let (endpoint, gate, task) = fresh_product_server(false, false, false).await;
    let mut client = AppServerClient::connect_product_http(
        &endpoint,
        "private-token".into(),
        "computer-a".into(),
        None,
    )
    .unwrap();
    let data = client
        .request_value("model/list", ModelListRequest { node_id: None })
        .await
        .unwrap();
    assert_eq!(data["source"], "fresh-server");
    assert_eq!(gate.bootstraps.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(gate.attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn fresh_timeline_bootstraps_without_losing_subscription_scope_or_events() {
    let (endpoint, gate, task) = fresh_product_server(false, false, false).await;
    let mut client = AppServerClient::connect_product_http(
        &endpoint,
        "private-token".into(),
        "computer-a".into(),
        None,
    )
    .unwrap();
    let mut stream = client
        .open_timeline_stream(SubscribeTimelineRequest {
            session_id: "session-1".into(),
            turn_id: None,
            since_event_id: None,
            replay_limit: Some(10),
            close_when_idle: Some(false),
            close_on_terminal: None,
        })
        .await
        .unwrap();
    assert_eq!(
        stream.stream.recv().await.unwrap(),
        timeline_event("fresh-server-event", 1)
    );
    assert_eq!(gate.bootstraps.load(std::sync::atomic::Ordering::SeqCst), 1);
    client.shutdown().await.unwrap();
    task.abort();
}

#[tokio::test]
async fn fresh_source_range_bootstraps_without_changing_peer_or_byte_range() {
    let (endpoint, gate, task) = fresh_product_server(true, false, false).await;
    let client = AppServerClient::connect_source_product_http(
        &endpoint,
        "private-token".into(),
        "source-a".into(),
        None,
    )
    .unwrap();
    assert_eq!(
        client
            .read_product_file_range("file-a", 2..5)
            .await
            .unwrap()
            .data,
        Bytes::from_static(b"cde")
    );
    assert_eq!(gate.bootstraps.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(gate.attempts.load(std::sync::atomic::Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn account_bootstrap_failure_or_second_unauthorized_response_never_loops() {
    for reject_bootstrap in [false, true] {
        let (endpoint, gate, task) = fresh_product_server(false, reject_bootstrap, true).await;
        let mut client = AppServerClient::connect_product_http(
            &endpoint,
            "private-token".into(),
            "computer-a".into(),
            None,
        )
        .unwrap();
        assert!(client
            .request_value("model/list", ModelListRequest { node_id: None })
            .await
            .unwrap_err()
            .is_code(talos_error::ErrorCode::Unauthenticated));
        assert_eq!(gate.bootstraps.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            gate.attempts.load(std::sync::atomic::Ordering::SeqCst),
            if reject_bootstrap { 1 } else { 2 }
        );
        task.abort();
    }
}
