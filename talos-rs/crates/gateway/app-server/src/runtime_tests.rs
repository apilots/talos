use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::json;
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentTurnRef, AgentTurnStarted, TimelineCursorStatus,
    TimelineEvent, TimelineReplay,
};
use talos_app_protocol::errors::{NOT_INITIALIZED, PARSE_ERROR};
use talos_app_protocol::requests::ActorCredentials;
use talos_service::{
    Actor, TimelineSubscriptionCloseReason, TimelineSubscriptionClosed, TimelineSubscriptionMessage,
};
use talos_types::SubscribeTimelineRequest;
use tokio::io::{duplex, split, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{broadcast, mpsc};

use super::*;
use crate::application::{ApplicationResponse, ApplicationResult};
use crate::auth::ActorCredentialValidator;

#[derive(Default)]
struct MockApp {
    desktop_host: Option<Arc<talos_browser::NativeBrowser>>,
    dispatched: Mutex<Vec<String>>,
    dispatched_actors: Mutex<Vec<String>>,
    dispatched_auth_disabled: Mutex<Vec<bool>>,
    session_sources: Mutex<Vec<talos_types::talos::SessionSource>>,
    cancelled: Mutex<Vec<String>>,
    cancelled_turns: Mutex<Vec<String>>,
    cancelled_actors: Mutex<Vec<String>>,
    drained: Mutex<usize>,
    live_tx: Mutex<Option<broadcast::Sender<TimelineEvent>>>,
    stream_txs: Mutex<Vec<mpsc::Sender<TimelineSubscriptionMessage>>>,
}

#[async_trait]
impl AppServerApplication for MockApp {
    async fn dispatch(
        &self,
        actor: &Actor,
        request: talos_app_protocol::requests::ClientRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        self.dispatched
            .lock()
            .expect("dispatch lock")
            .push(request.method.as_str().to_string());
        self.dispatched_actors
            .lock()
            .expect("dispatch actor lock")
            .push(actor.user_id.clone());
        self.dispatched_auth_disabled
            .lock()
            .expect("dispatch auth-disabled lock")
            .push(actor.auth_disabled);
        let (body, started_turn) = match request.params {
            ClientRequestParams::Browser(_) if self.desktop_host.is_some() => {
                use talos_browser::BrowserBackend;
                self.desktop_host
                    .as_ref()
                    .expect("host")
                    .open_local(actor.user_id.clone())
                    .await
                    .map_err(|error| error.to_api_error())?;
                (json!({"ok":true}), None)
            }
            ClientRequestParams::TurnStart(params) => (
                json!({
                    "requestId": "req-1",
                    "messageId": "msg-1",
                    "sessionId": params.session_id,
                    "turnId": "turn-1",
                    "status": "accepted",
                }),
                Some(AgentTurnRef::new("s1", "turn-1")),
            ),
            _ => (json!({ "ok": true }), None),
        };
        Ok(ApplicationResponse { body, started_turn })
    }

    async fn start_turn(
        &self,
        actor: &Actor,
        source: talos_types::talos::SessionSource,
        request: talos_types::StartTurnRequest,
    ) -> ApplicationResult<ApplicationResponse> {
        self.dispatched
            .lock()
            .expect("dispatch lock")
            .push("turn/start".to_string());
        self.dispatched_actors
            .lock()
            .expect("dispatch actor lock")
            .push(actor.user_id.clone());
        self.dispatched_auth_disabled
            .lock()
            .expect("dispatch auth-disabled lock")
            .push(actor.auth_disabled);
        self.session_sources
            .lock()
            .expect("session source lock")
            .push(source);
        Ok(ApplicationResponse {
            body: json!({
                "requestId": "req-1",
                "messageId": "msg-1",
                "sessionId": request.session_id,
                "turnId": "turn-1",
                "status": "accepted",
            }),
            started_turn: Some(AgentTurnRef::new("s1", "turn-1")),
        })
    }

    async fn subscribe_timeline_events(
        &self,
        _actor: &Actor,
        params: SubscribeTimelineRequest,
    ) -> ApplicationResult<talos_service::TimelineSubscription> {
        let live_tx = self.live_tx.lock().expect("live tx lock").as_ref().cloned();
        match live_tx {
            Some(_live_tx) => {
                let (_tx, events) = mpsc::channel(16);
                let subscription_id = {
                    let mut stream_txs = self.stream_txs.lock().expect("stream tx lock");
                    let subscription_id = format!("sub_mock_{}", stream_txs.len() + 1);
                    stream_txs.push(_tx);
                    subscription_id
                };
                Ok(talos_service::TimelineSubscription {
                    subscription_id,
                    session_id: params.session_id,
                    high_water_mark_event_id: None,
                    replay: TimelineReplay {
                        events: Vec::new(),
                        high_water_mark_event_id: None,
                        next_cursor: None,
                        last_event_seq: 0,
                        cursor_status: TimelineCursorStatus::FromBeginning,
                    },
                    events,
                    has_live_turn: false,
                })
            }
            None => Err(ApiError::new(ErrorCode::ResourceNotFound)),
        }
    }

    async fn cancel_active_turn(
        &self,
        actor: &Actor,
        session_id: &str,
        turn_id: &str,
    ) -> ApplicationResult<()> {
        self.cancelled
            .lock()
            .expect("cancel lock")
            .push(session_id.to_string());
        self.cancelled_turns
            .lock()
            .expect("cancel turn lock")
            .push(turn_id.to_string());
        self.cancelled_actors
            .lock()
            .expect("cancel actor lock")
            .push(actor.user_id.clone());
        Ok(())
    }

    async fn drain_background_tasks(
        &self,
        _timeout: std::time::Duration,
    ) -> talos_error::TalosResult<()> {
        *self.drained.lock().expect("drain lock") += 1;
        Ok(())
    }
}

struct RejectingValidator;

#[async_trait]
impl ActorCredentialValidator for RejectingValidator {
    async fn validate(&self, _credentials: &ActorCredentials) -> ApplicationResult<Actor> {
        Err(ApiError::new(ErrorCode::Unauthenticated))
    }
}

struct ExistingTokenValidator;

#[async_trait]
impl ActorCredentialValidator for ExistingTokenValidator {
    async fn validate(&self, credentials: &ActorCredentials) -> ApplicationResult<Actor> {
        match &credentials.credential {
            talos_app_protocol::requests::ActorCredential::ExistingSessionToken { token }
                if token == "user-token" =>
            {
                Ok(Actor::regular("user-1"))
            }
            _ => Err(ApiError::new(ErrorCode::Unauthenticated)),
        }
    }
}

#[tokio::test]
async fn incompatible_protocol_is_rejected_before_business_dispatch() {
    let mut request: serde_json::Value =
        serde_json::from_str(&initialize_request("secret")).unwrap();
    request["params"]["capabilities"]["protocol_version"] = json!({"major": 0, "minor": 3});
    let output = run_with_input(format!("{request}\n{}", session_read_request())).await;
    let messages = parse_output(&output);
    assert_eq!(
        messages[0]["error"]["code"],
        talos_app_protocol::errors::INVALID_PARAMS.as_i32()
    );
    assert_eq!(messages[1]["error"]["code"], NOT_INITIALIZED.as_i32());
}

#[tokio::test]
async fn request_before_initialize_returns_not_initialized() {
    let (output, log) = capture_runtime_logs(run_with_input(session_read_request())).await;
    let messages = parse_output(&output);

    assert_eq!(messages[0]["error"]["code"], NOT_INITIALIZED.as_i32());
    assert_eq!(log.matches("application failure").count(), 1);
    assert!(log.contains("backtrace_status=Captured"));
}

#[tokio::test]
async fn malformed_line_returns_parse_error_and_keeps_running() {
    let (output, log) = capture_runtime_logs(run_with_input(format!(
        "{{\n{}",
        initialize_request("secret")
    )))
    .await;
    let messages = parse_output(&output);

    assert_eq!(messages[0]["error"]["code"], PARSE_ERROR.as_i32());
    assert_eq!(result_data(&messages[1])["protocol_version"]["major"], 0);
    assert_eq!(log.matches("application failure").count(), 1);
    assert!(log.contains("backtrace_status=Captured"));
}

async fn capture_runtime_logs<T>(operation: impl std::future::Future<Output = T>) -> (T, String) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("runtime.log");
    // Nextest runs each test in its own process, matching the production logger lifetime.
    let guard = talos_logging::init_with_config(talos_logging::LoggingConfig {
        file: Some(path.to_string_lossy().into_owned()),
        stdout: false,
        ..Default::default()
    })
    .unwrap();
    let result = operation.await;
    drop(guard);
    (result, std::fs::read_to_string(path).unwrap())
}

#[tokio::test]
async fn incoming_jsonl_frame_respects_the_configured_byte_limit() {
    let initialize = initialize_request("secret");
    let max_incoming_frame_bytes = initialize.len();
    let (output, log) = capture_runtime_logs(run_with_app_validator_and_frame_limit(
        format!("{initialize}{}\n", "x".repeat(max_incoming_frame_bytes + 1)),
        Arc::new(MockApp::default()),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
        max_incoming_frame_bytes,
    ))
    .await;
    let messages = parse_output(&output);

    assert_eq!(result_data(&messages[0])["protocol_version"]["major"], 0);
    assert_eq!(messages[1]["error"]["code"], PARSE_ERROR.as_i32());
    assert_eq!(log.matches("application failure").count(), 1);
    assert!(log.contains("backtrace_status=Captured"));
}

#[tokio::test]
async fn invalid_utf8_is_reported_once_and_keeps_the_numeric_protocol_envelope() {
    let (output, log) = capture_runtime_logs(run_with_reader(&b"\xff\n"[..])).await;
    assert_eq!(
        parse_output(&output),
        vec![json!({
            "jsonrpc": "2.0", "id": null,
            "error": { "code": PARSE_ERROR.as_i32(), "message": "The app-server request is not valid UTF-8." }
        })]
    );
    assert_eq!(log.matches("application failure").count(), 1);
    assert!(log.contains("backtrace_status=Captured"));
    assert!(log.contains("sources=0: invalid utf-8"));
}

#[tokio::test]
async fn input_io_failure_retains_its_private_source_without_disclosing_it() {
    struct FailedReader;
    impl AsyncRead for FailedReader {
        fn poll_read(
            self: std::pin::Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::task::Poll::Ready(Err(std::io::Error::other("private-input-sentinel")))
        }
    }
    let (output, log) = capture_runtime_logs(run_with_reader(FailedReader)).await;
    assert_eq!(
        parse_output(&output),
        vec![json!({
            "jsonrpc": "2.0", "id": null,
            "error": { "code": PARSE_ERROR.as_i32(), "message": "Failed to read the app-server request stream." }
        })]
    );
    assert_eq!(log.matches("application failure").count(), 1);
    assert!(log.contains("backtrace_status=Captured"));
    assert!(log.contains("sources=0: private-input-sentinel"));
    assert!(log.contains("Internal"));
}

async fn run_with_reader(reader: impl AsyncRead + Unpin) -> String {
    let (mut client, writer) = duplex(16 * 1024);
    let mut output = String::new();
    let (result, read) = tokio::join!(
        run_jsonl(
            reader,
            writer,
            Arc::new(MockApp::default()),
            Arc::new(ExistingTokenValidator),
            AppServerConfig::default()
        ),
        client.read_to_string(&mut output),
    );
    result.unwrap();
    read.unwrap();
    output
}

#[tokio::test]
async fn invalid_credentials_return_unauthorized() {
    let output = run_with_app_validator_and_config(
        format!(
            "{}{}",
            initialize_request("secret"),
            session_read_request_with_actor_token("bad")
        ),
        Arc::new(MockApp::default()),
        Arc::new(RejectingValidator),
        AppServerConfig {
            auth_enabled: true,
            ..AppServerConfig::default()
        },
    )
    .await;
    let messages = parse_output(&output);

    assert_eq!(result_error(&messages[1])["code"], "unauthenticated");
}

#[tokio::test]
async fn dispatches_request_after_initialize() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app(
        format!("{}{}", initialize_request("secret"), session_read_request()),
        app.clone(),
    )
    .await;
    let messages = parse_output(&output);

    assert!(result_data(&messages[1])["ok"].as_bool().unwrap_or(false));
    assert_eq!(
        app.dispatched.lock().expect("dispatch lock").as_slice(),
        &["session/get"]
    );
}

#[tokio::test]
async fn auth_disabled_dispatches_business_requests_as_anonymous() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app(
        format!("{}{}", initialize_request("secret"), session_read_request()),
        app.clone(),
    )
    .await;
    let messages = parse_output(&output);

    assert!(result_data(&messages[1])["ok"].as_bool().unwrap_or(false));
    assert_eq!(
        app.dispatched_actors
            .lock()
            .expect("dispatch actor lock")
            .as_slice(),
        &["anonymous"]
    );
    assert_eq!(
        app.dispatched_auth_disabled
            .lock()
            .expect("dispatch auth-disabled lock")
            .as_slice(),
        &[true]
    );
}

#[tokio::test]
async fn auth_enabled_requires_request_actor_credentials_after_initialize() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app_validator_and_config(
        format!("{}{}", initialize_request("secret"), session_read_request()),
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig {
            auth_enabled: true,
            ..AppServerConfig::default()
        },
    )
    .await;
    let messages = parse_output(&output);

    assert_eq!(result_error(&messages[1])["code"], "unauthenticated");
    assert!(app.dispatched.lock().expect("dispatch lock").is_empty());
}

#[tokio::test]
async fn auth_enabled_allows_session_cache_read_after_local_initialize() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app_validator_and_config(
        format!(
            "{}{}",
            initialize_request("secret"),
            auth_session_read_request()
        ),
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig {
            auth_enabled: true,
            ..AppServerConfig::default()
        },
    )
    .await;
    let messages = parse_output(&output);

    assert!(result_data(&messages[1])["ok"].as_bool().unwrap_or(false));
    assert_eq!(
        app.dispatched_actors
            .lock()
            .expect("dispatch actor lock")
            .as_slice(),
        &["anonymous"]
    );
}

#[tokio::test]
async fn auth_enabled_dispatches_with_existing_session_token_credentials() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app_validator_and_config(
        format!(
            "{}{}",
            initialize_request("secret"),
            session_read_request_with_actor_token("user-token")
        ),
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig {
            auth_enabled: true,
            ..AppServerConfig::default()
        },
    )
    .await;
    let messages = parse_output(&output);

    assert!(result_data(&messages[1])["ok"].as_bool().unwrap_or(false));
    assert_eq!(
        app.dispatched.lock().expect("dispatch lock").as_slice(),
        &["session/get"]
    );
    assert_eq!(
        app.dispatched_actors
            .lock()
            .expect("dispatch actor lock")
            .as_slice(),
        &["user-1"]
    );
}

#[tokio::test]
async fn owned_turn_shutdown_uses_business_request_actor() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app(
        format!("{}{}", initialize_request("secret"), turn_start_request()),
        app.clone(),
    )
    .await;
    let messages = parse_output(&output);

    assert_eq!(result_data(&messages[1])["sessionId"], "s1");
    assert_eq!(
        app.cancelled.lock().expect("cancel lock").as_slice(),
        &["s1"]
    );
    assert_eq!(
        app.cancelled_turns
            .lock()
            .expect("cancel turn lock")
            .as_slice(),
        &["turn-1"]
    );
    assert_eq!(
        app.cancelled_actors
            .lock()
            .expect("cancel actor lock")
            .as_slice(),
        &["anonymous"]
    );
}

#[tokio::test]
async fn turn_start_uses_initialized_client_kind_as_session_source() {
    let app = Arc::new(MockApp::default());
    let output = run_with_app(
        format!(
            "{}{}",
            initialize_request_for_kind("desktop"),
            turn_start_request()
        ),
        app.clone(),
    )
    .await;
    let messages = parse_output(&output);

    assert_eq!(result_data(&messages[1])["sessionId"], "s1");
    assert_eq!(
        app.session_sources
            .lock()
            .expect("session source lock")
            .as_slice(),
        &[talos_types::talos::SessionSource::desktop()]
    );
}

#[tokio::test]
async fn idle_subscription_does_not_block_shutdown_after_background_drain() {
    let app = Arc::new(MockApp::default());
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().expect("live tx lock") = Some(live_tx);

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        run_with_app(
            format!(
                "{}{}{}",
                initialize_request("secret"),
                initialized_notification(),
                event_subscribe_request()
            ),
            app.clone(),
        ),
    )
    .await
    .expect("run_jsonl should not hang on idle subscription");
    let messages = parse_output(&output);

    assert_eq!(result_data(&messages[1])["sessionId"], "s1");
    assert_eq!(*app.drained.lock().expect("drain lock"), 1);
}

#[tokio::test]
async fn event_unsubscribe_stops_only_target_subscription() {
    let app = Arc::new(MockApp::default());
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().expect("live tx lock") = Some(live_tx);

    let (client, server) = duplex(16 * 1024);
    let (server_read, server_write) = split(server);
    let server_task = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let (client_read, mut client_write) = split(client);
    let mut output = BufReader::new(client_read).lines();

    client_write
        .write_all(
            format!(
                "{}{}{}{}",
                initialize_request("secret"),
                initialized_notification(),
                event_subscribe_request_with_id(2),
                event_subscribe_request_with_id(3)
            )
            .as_bytes(),
        )
        .await
        .expect("write requests");

    let initialize = read_output_line(&mut output).await;
    let first_subscribe = read_output_line(&mut output).await;
    let second_subscribe = read_output_line(&mut output).await;
    assert_eq!(result_data(&initialize)["protocol_version"]["major"], 0);
    assert_eq!(result_data(&first_subscribe)["sessionId"], "s1");
    assert_eq!(result_data(&second_subscribe)["sessionId"], "s1");

    let first_subscription_id = result_data(&first_subscribe)["subscriptionId"]
        .as_str()
        .expect("first subscription id")
        .to_string();
    let second_subscription_id = result_data(&second_subscribe)["subscriptionId"]
        .as_str()
        .expect("second subscription id")
        .to_string();
    assert_ne!(first_subscription_id, second_subscription_id);

    let senders = app.stream_txs.lock().expect("stream tx lock").clone();
    assert_eq!(senders.len(), 2);
    senders[0]
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_1", 1),
        )))
        .await
        .expect("send first subscription event");
    senders[1]
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_2", 2),
        )))
        .await
        .expect("send second subscription event");

    let first_notification = read_output_line(&mut output).await;
    let second_notification = read_output_line(&mut output).await;
    assert_eq!(
        first_notification["params"]["subscriptionId"],
        first_subscription_id
    );
    assert_eq!(first_notification["params"]["event"]["eventId"], "ev_1");
    assert_eq!(
        second_notification["params"]["subscriptionId"],
        second_subscription_id
    );
    assert_eq!(second_notification["params"]["event"]["eventId"], "ev_2");

    client_write
        .write_all(event_unsubscribe_request(4, &first_subscription_id).as_bytes())
        .await
        .expect("write unsubscribe request");
    let unsubscribe = read_output_line(&mut output).await;
    assert_eq!(
        result_data(&unsubscribe)["subscriptionId"],
        first_subscription_id
    );
    assert!(result_data(&unsubscribe)["unsubscribed"]
        .as_bool()
        .unwrap_or(false));

    assert!(senders[0]
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_3", 3)
        )))
        .await
        .is_err());
    senders[1]
        .send(TimelineSubscriptionMessage::Event(Box::new(
            timeline_event("ev_4", 4),
        )))
        .await
        .expect("second subscription should remain active");
    let remaining_notification = read_output_line(&mut output).await;
    assert_eq!(
        remaining_notification["params"]["subscriptionId"],
        second_subscription_id
    );
    assert_eq!(remaining_notification["params"]["event"]["eventId"], "ev_4");

    client_write.shutdown().await.expect("shutdown input");
    server_task.await.expect("server join").expect("server run");
}

#[tokio::test]
async fn closed_subscription_sends_completed_notification() {
    let app = Arc::new(MockApp::default());
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().expect("live tx lock") = Some(live_tx);

    let (client, server) = duplex(16 * 1024);
    let (server_read, server_write) = split(server);
    let server_task = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let (client_read, mut client_write) = split(client);
    let mut output = BufReader::new(client_read).lines();

    client_write
        .write_all(
            format!(
                "{}{}{}",
                initialize_request("secret"),
                initialized_notification(),
                event_subscribe_request()
            )
            .as_bytes(),
        )
        .await
        .expect("write requests");

    let _initialize = read_output_line(&mut output).await;
    let subscribe = read_output_line(&mut output).await;
    let subscription_id = result_data(&subscribe)["subscriptionId"]
        .as_str()
        .expect("subscription id")
        .to_string();
    let sender = app
        .stream_txs
        .lock()
        .expect("stream tx lock")
        .first()
        .cloned()
        .expect("subscription sender");
    sender
        .send(TimelineSubscriptionMessage::Closed(
            TimelineSubscriptionClosed {
                reason: TimelineSubscriptionCloseReason::Completed,
                message: "event subscription completed".to_string(),
            },
        ))
        .await
        .expect("send completed close");

    let notification = read_output_line(&mut output).await;
    assert_eq!(notification["method"], "subscription/terminated");
    assert_eq!(notification["params"]["subscriptionId"], subscription_id);
    assert_eq!(notification["params"]["sessionId"], "s1");
    assert_eq!(notification["params"]["reason"], "completed");

    client_write.shutdown().await.expect("shutdown input");
    server_task.await.expect("server join").expect("server run");
}

#[tokio::test]
async fn message_queue_change_uses_live_notification_without_subscription_envelope() {
    let app = Arc::new(MockApp::default());
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().expect("live tx lock") = Some(live_tx);

    let (client, server) = duplex(16 * 1024);
    let (server_read, server_write) = split(server);
    let server_task = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let (client_read, mut client_write) = split(client);
    let mut output = BufReader::new(client_read).lines();

    client_write
        .write_all(
            format!(
                "{}{}{}",
                initialize_request("secret"),
                initialized_notification(),
                event_subscribe_request()
            )
            .as_bytes(),
        )
        .await
        .expect("write requests");

    let _initialize = read_output_line(&mut output).await;
    let _subscribe = read_output_line(&mut output).await;
    let sender = app
        .stream_txs
        .lock()
        .expect("stream tx lock")
        .first()
        .cloned()
        .expect("subscription sender");
    sender
        .send(TimelineSubscriptionMessage::MessageQueueChanged(
            talos_types::MessageQueueChangedNotification {
                session_id: "s1".to_string(),
                revision: 7,
            },
        ))
        .await
        .expect("send message queue change");

    let notification = read_output_line(&mut output).await;
    assert_eq!(notification["method"], "messageQueue/changed");
    assert_eq!(
        notification["params"],
        json!({ "sessionId": "s1", "revision": 7 })
    );
    assert!(notification["params"].get("subscriptionId").is_none());

    client_write.shutdown().await.expect("shutdown input");
    server_task.await.expect("server join").expect("server run");
}

#[tokio::test]
async fn lagged_subscription_close_is_not_reported_as_completed() {
    let app = Arc::new(MockApp::default());
    let (live_tx, _live_rx) = broadcast::channel(16);
    *app.live_tx.lock().expect("live tx lock") = Some(live_tx);

    let (client, server) = duplex(16 * 1024);
    let (server_read, server_write) = split(server);
    let server_task = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app.clone(),
        Arc::new(ExistingTokenValidator),
        AppServerConfig::default(),
    ));
    let (client_read, mut client_write) = split(client);
    let mut output = BufReader::new(client_read).lines();

    client_write
        .write_all(
            format!(
                "{}{}{}",
                initialize_request("secret"),
                initialized_notification(),
                event_subscribe_request()
            )
            .as_bytes(),
        )
        .await
        .expect("write requests");

    let _initialize = read_output_line(&mut output).await;
    let subscribe = read_output_line(&mut output).await;
    let subscription_id = result_data(&subscribe)["subscriptionId"]
        .as_str()
        .expect("subscription id")
        .to_string();
    let sender = app
        .stream_txs
        .lock()
        .expect("stream tx lock")
        .first()
        .cloned()
        .expect("subscription sender");
    sender
        .send(TimelineSubscriptionMessage::Closed(
            TimelineSubscriptionClosed {
                reason: TimelineSubscriptionCloseReason::Lagged,
                message: "subscription lagged behind retained live events".to_string(),
            },
        ))
        .await
        .expect("send lagged close");

    let notification = read_output_line(&mut output).await;
    assert_eq!(notification["method"], "subscription/terminated");
    assert_eq!(notification["params"]["subscriptionId"], subscription_id);
    assert_eq!(notification["params"]["reason"], "lagged");

    client_write.shutdown().await.expect("shutdown input");
    server_task.await.expect("server join").expect("server run");
}

async fn run_with_input(input: String) -> String {
    run_with_app(input, Arc::new(MockApp::default())).await
}

async fn run_with_app(input: String, app: Arc<MockApp>) -> String {
    run_with_app_and_validator(input, app, Arc::new(ExistingTokenValidator)).await
}

async fn run_with_app_and_validator(
    input: String,
    app: Arc<MockApp>,
    validator: Arc<dyn ActorCredentialValidator>,
) -> String {
    run_with_app_validator_and_config(input, app, validator, AppServerConfig::default()).await
}

async fn run_with_app_validator_and_config(
    input: String,
    app: Arc<MockApp>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
) -> String {
    run_with_app_validator_and_frame_limit(
        input,
        app,
        validator,
        config,
        DEFAULT_MAX_INCOMING_FRAME_BYTES,
    )
    .await
}

async fn run_with_app_validator_and_frame_limit(
    input: String,
    app: Arc<MockApp>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
    max_incoming_frame_bytes: usize,
) -> String {
    let (mut client, server) = duplex(16 * 1024);
    let (server_read, server_write) = split(server);
    let server_task = tokio::spawn(run_jsonl_with_max_frame_bytes(
        server_read,
        server_write,
        app,
        validator,
        config,
        max_incoming_frame_bytes,
    ));
    client
        .write_all(input.as_bytes())
        .await
        .expect("write input");
    client.shutdown().await.expect("shutdown input");
    let mut output = String::new();
    client
        .read_to_string(&mut output)
        .await
        .expect("read output");
    server_task.await.expect("server join").expect("server run");
    output
}

fn initialize_request(_token: &str) -> String {
    initialize_request_for_kind("sdk")
}

fn initialize_request_for_kind(kind: &str) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "client_info": {
                    "name": "test",
                    "version": "0.1.5",
                    "kind": kind
                },
                "capabilities": {
                    "protocol_version": ProtocolVersion::CURRENT,
                    "supports_server_requests": false,
                    "supports_event_replay": true
                }
            }
        })
    )
}

fn session_read_request() -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/get",
            "params": { "sessionId": "s1" }
        })
    )
}

fn session_read_request_with_actor_token(token: &str) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "session/get",
            "actor_credentials": {
                "credential": {
                    "type": "existing_session_token",
                    "token": token
                }
            },
            "params": { "sessionId": "s1" }
        })
    )
}

fn auth_session_read_request() -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "auth/session/read",
            "params": {}
        })
    )
}

fn turn_start_request() -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "turn/start",
            "params": {
                "sessionId": "s1",
                "input": {
                    "content": [{ "type": "text", "text": "hello" }]
                },
                "modelSelection": null,
                "userPermissionMode": null,
                "projectId": null,
                "currentDirectory": null,
                "clientRequestId": "request-1"
            }
        })
    )
}

fn initialized_notification() -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "method": "initialized"
        })
    )
}

fn event_subscribe_request() -> String {
    event_subscribe_request_with_id(2)
}

fn event_subscribe_request_with_id(id: u64) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "event/subscribe",
            "params": {
                "sessionId": "s1",
                "turnId": null,
                "sinceEventId": null,
                "replayLimit": 1,
                "closeWhenIdle": null,
                "closeOnTerminal": null
            }
        })
    )
}

fn event_unsubscribe_request(id: u64, subscription_id: &str) -> String {
    format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "event/unsubscribe",
            "params": {
                "subscriptionId": subscription_id
            }
        })
    )
}

async fn read_output_line<R>(output: &mut tokio::io::Lines<BufReader<R>>) -> serde_json::Value
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = output
        .next_line()
        .await
        .expect("read output line")
        .expect("output line");
    serde_json::from_str(&line).expect("json output line")
}

fn timeline_event(event_id: &str, event_seq: i64) -> TimelineEvent {
    let event = AgentEvent {
        identity: AgentTurnRef::new("s1", "turn-1"),
        sequence: event_seq as u64,
        occurred_at: 100 + event_seq,
        payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
    };
    TimelineEvent {
        event_id: event_id.to_string(),
        event_seq,
        persisted_at: 100 + event_seq,
        event: talos_agent_api::TimelineEventPayload::Agent(event),
    }
}

fn parse_output(output: &str) -> Vec<serde_json::Value> {
    output
        .lines()
        .map(|line| serde_json::from_str(line).expect("json line"))
        .collect()
}

fn result_data(message: &serde_json::Value) -> &serde_json::Value {
    assert_eq!(message["result"]["status"], "success");
    &message["result"]["data"]
}

fn result_error(message: &serde_json::Value) -> &serde_json::Value {
    assert_eq!(message["result"]["status"], "error");
    &message["result"]["error"]
}
#[tokio::test]
async fn disconnected_output_retains_the_io_source_as_an_internal_failure() {
    let (writer, reader) = tokio::io::duplex(64);
    drop(reader);
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tx.send(super::OutgoingMessage::Notification(
        talos_app_protocol::jsonrpc::JsonRpcNotification::new("fixture", None),
    ))
    .await
    .unwrap();
    drop(tx);
    let error = super::write_loop(writer, rx).await.unwrap_err();
    assert_eq!(error.error_code(), talos_error::ErrorCode::InternalError);
    assert!(std::error::Error::source(&error)
        .unwrap()
        .is::<std::io::Error>());
}

#[tokio::test]
async fn desktop_browser_requests_cross_jsonl_and_correlate_only_the_original_response() {
    use talos_browser::BrowserBackend;
    let host = crate::DesktopHostBridge::new();
    let backend = talos_browser::NativeBrowser::new(host.clone());
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        Arc::new(MockApp::default()),
        Arc::new(RejectingValidator),
        AppServerConfig {
            desktop_host: Some(host),
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(desktop_host_initialize_request().as_bytes())
        .await
        .expect("initialize");
    let initialized = read_output_line(&mut output).await;
    assert!(initialized.get("result").is_some());
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !matches!(
            backend.readiness(talos_types::BrowserTargetKind::Managed),
            talos_types::BrowserReadiness::Ready
        ) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("trusted Desktop initialized");
    let request = tokio::spawn(async move { backend.open_local("alice".into()).await });
    let outgoing = read_output_line(&mut output).await;
    assert_eq!(outgoing["method"], "client/browser/execute");
    assert_eq!(outgoing["params"]["userId"], "alice");
    assert_eq!(outgoing["params"]["operation"]["kind"], "show");
    let wrong = json!({"jsonrpc":"2.0","id":"wrong-response","result":{"kind":"done"}});
    client_write
        .write_all(format!("{wrong}\n").as_bytes())
        .await
        .expect("wrong response");
    tokio::task::yield_now().await;
    assert!(!request.is_finished());
    let response = json!({"jsonrpc":"2.0","id":outgoing["id"],"result":{"kind":"done"}});
    client_write
        .write_all(format!("{response}\n").as_bytes())
        .await
        .expect("response");
    request
        .await
        .expect("request task")
        .expect("correlated response");
    client_write.shutdown().await.expect("shutdown input");
    runtime
        .await
        .expect("runtime task")
        .expect("runtime shutdown");
}

#[tokio::test]
async fn serialized_business_dispatch_does_not_block_its_own_host_request_or_response() {
    let host = crate::DesktopHostBridge::new();
    let app = Arc::new(MockApp {
        desktop_host: Some(talos_browser::NativeBrowser::new(host.clone())),
        ..Default::default()
    });
    let (client, server) = duplex(128 * 1024);
    let (client_read, mut client_write) = split(client);
    let (server_read, server_write) = split(server);
    let runtime = tokio::spawn(run_jsonl(
        server_read,
        server_write,
        app,
        Arc::new(RejectingValidator),
        AppServerConfig {
            desktop_host: Some(host),
            ..Default::default()
        },
    ));
    let mut output = BufReader::new(client_read).lines();
    client_write
        .write_all(desktop_host_initialize_request().as_bytes())
        .await
        .expect("initialize");
    read_output_line(&mut output).await;
    let browser = json!({"jsonrpc":"2.0","id":2,"method":"browser/operate","params":{"nodeId":"embedded","operation":{"kind":"read"}}});
    client_write
        .write_all(format!("{browser}\n").as_bytes())
        .await
        .expect("browser read");
    let outgoing = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_output_line(&mut output),
    )
    .await
    .expect("host request while business dispatch waits");
    assert_eq!(outgoing["method"], "client/browser/execute");
    for id in 3..7 {
        let queued = json!({"jsonrpc":"2.0","id":id,"method":"session/read","params":{"sessionId":"queued"}});
        client_write
            .write_all(format!("{queued}\n").as_bytes())
            .await
            .expect("queue request");
    }
    let response = json!({"jsonrpc":"2.0","id":outgoing["id"],"result":{"kind":"done"}});
    client_write
        .write_all(format!("{response}\n").as_bytes())
        .await
        .expect("host response");
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        read_output_line(&mut output),
    )
    .await
    .expect("business response");
    assert_eq!(result["id"], 2);
    assert!(result.get("result").is_some());
    for id in 3..7 {
        assert_eq!(read_output_line(&mut output).await["id"], id);
    }
    client_write.shutdown().await.expect("shutdown");
    runtime.await.expect("runtime").expect("result");
}

fn desktop_host_initialize_request() -> String {
    let mut request: serde_json::Value =
        serde_json::from_str(&initialize_request("secret")).expect("initialize");
    request["params"]["client_info"]["kind"] = json!("desktop");
    format!("{request}\n{}", initialized_notification())
}
