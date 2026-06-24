use std::collections::HashMap;
use std::sync::Arc;

use talos_app_protocol::errors::{AppServerErrorBody, ALREADY_INITIALIZED, UNAUTHORIZED};
use talos_app_protocol::events::{
    canonical_method, SubscriptionTerminatedEnvelope, SubscriptionTerminatedReason,
    TimelineSubscriptionNotification,
};
use talos_app_protocol::jsonrpc::{
    parse_message_line, JsonRpcErrorResponse, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use talos_app_protocol::requests::{
    ActorCredentials, ClientMethod, ClientRequest, ClientRequestParams, InitializeResponse,
    ServerInfo,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_service::{
    Actor, TimelineSubscription, TimelineSubscriptionCloseReason, TimelineSubscriptionMessage,
};
use talos_types::EventUnsubscribeResponse;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::application::{AppServerApplication, ProtocolResult};
use crate::auth::ActorCredentialValidator;

#[derive(Debug, Clone)]
pub struct AppServerConfig {
    pub outgoing_capacity: usize,
    pub subscription_capacity: usize,
    pub server_name: String,
    pub server_version: String,
    pub auth_enabled: bool,
    pub cloud_auth_url: Option<String>,
}

impl Default for AppServerConfig {
    fn default() -> Self {
        Self {
            outgoing_capacity: 256,
            subscription_capacity: 256,
            server_name: "talos-app-server".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            auth_enabled: false,
            cloud_auth_url: None,
        }
    }
}

enum OutgoingMessage {
    Response(JsonRpcResponse),
    Error(JsonRpcErrorResponse),
    Notification(JsonRpcNotification),
}

pub async fn run_stdio(
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
) -> ProtocolResult<()> {
    run_jsonl(
        tokio::io::stdin(),
        tokio::io::stdout(),
        app,
        validator,
        config,
    )
    .await
}

pub async fn run_jsonl<R, W>(
    reader: R,
    writer: W,
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
) -> ProtocolResult<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (outgoing_tx, outgoing_rx) = mpsc::channel(config.outgoing_capacity.max(1));
    let mut writer_task = tokio::spawn(write_loop(writer, outgoing_rx));
    let mut state = RuntimeState::default();
    let mut lines = BufReader::new(reader).lines();
    let mut writer_finished = false;

    loop {
        tokio::select! {
            line = lines.next_line() => {
                match line {
                    Ok(Some(line)) => {
                        if handle_line(
                            &line,
                            &mut state,
                            app.clone(),
                            validator.clone(),
                            &config,
                            outgoing_tx.clone(),
                        )
                        .await
                        .is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(err) => {
                        let _ = outgoing_tx
                            .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                                None,
                                AppServerErrorBody::parse_error(err.to_string()),
                            )))
                            .await;
                        break;
                    }
                }
            }
            writer_result = &mut writer_task => {
                match writer_result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => tracing::error!(error = ?err, "app-server stdout writer failed"),
                    Err(err) => tracing::error!(error = %err, "app-server stdout writer task failed"),
                }
                writer_finished = true;
                break;
            }
        }
    }

    cancel_owned_turns(app.clone(), &state.owned_turn_sessions).await;

    const DRAIN_BACKGROUND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    if let Err(err) = app.drain_background_tasks(DRAIN_BACKGROUND_TIMEOUT).await {
        tracing::warn!(error = ?err, "drain_background_tasks failed during shutdown");
    }

    // Keep subscriptions alive while background drains run so final turn events can still be
    // forwarded. After the drain boundary, abort them explicitly; idle subscriptions wait on
    // the TalosService stream and would otherwise keep stdio shutdown from completing.
    for (_, task) in state.subscription_tasks {
        task.abort();
        let _ = task.await;
    }

    drop(outgoing_tx);

    if !writer_finished {
        match writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::error!(error = ?err, "app-server stdout writer failed"),
            Err(err) => tracing::error!(error = %err, "app-server stdout writer task failed"),
        }
    }
    Ok(())
}

#[derive(Default)]
struct RuntimeState {
    actor: Option<Actor>,
    initialized_notification_seen: bool,
    owned_turn_sessions: HashMap<String, Actor>,
    subscription_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

async fn handle_line(
    line: &str,
    state: &mut RuntimeState,
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: &AppServerConfig,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let message = match parse_message_line(line) {
        Ok(message) => message,
        Err(error) => {
            return outgoing_tx
                .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                    None, error,
                )))
                .await
                .map_err(|_| ());
        }
    };

    match message {
        JsonRpcMessage::Request(request) => {
            handle_request(request, state, app, validator, config, outgoing_tx).await
        }
        JsonRpcMessage::Notification(notification) => {
            if notification.method == "initialized" {
                state.initialized_notification_seen = true;
            }
            Ok(())
        }
        JsonRpcMessage::Response(_) | JsonRpcMessage::Error(_) => Ok(()),
    }
}

async fn handle_request(
    request: JsonRpcRequest,
    state: &mut RuntimeState,
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: &AppServerConfig,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let id = request.id.clone();
    let request_actor_credentials = request.actor_credentials.clone();
    let decoded = ClientRequest::decode(&request.method, request.params)
        .map_err(|error| send_error(outgoing_tx.clone(), id.clone(), error));
    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(send) => return send.await,
    };

    if decoded.method != ClientMethod::Initialize && state.actor.is_none() {
        return send_error(outgoing_tx, id, AppServerErrorBody::not_initialized()).await;
    }

    match decoded.params {
        ClientRequestParams::Initialize(_params) => {
            if state.actor.is_some() {
                return send_error(
                    outgoing_tx,
                    id,
                    AppServerErrorBody::new(
                        ALREADY_INITIALIZED,
                        "app-server is already initialized",
                        None,
                    ),
                )
                .await;
            }
            state.actor = Some(Actor::regular("anonymous"));
            let result = serde_json::to_value(InitializeResponse {
                protocol_version: ProtocolVersion::CURRENT,
                server_info: ServerInfo {
                    name: config.server_name.clone(),
                    version: config.server_version.clone(),
                    auth_enabled: config.auth_enabled,
                    cloud_auth_url: config.cloud_auth_url.clone(),
                },
            })
            .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()));
            send_result(outgoing_tx, id, result).await
        }
        ClientRequestParams::EventSubscribe(params) => {
            if !state.initialized_notification_seen {
                return send_error(outgoing_tx, id, AppServerErrorBody::not_initialized()).await;
            }
            let actor = match resolve_request_actor(
                state,
                validator.as_ref(),
                config,
                decoded.method,
                request_actor_credentials,
            )
            .await
            {
                Ok(actor) => actor,
                Err(error) => return send_error(outgoing_tx, id, error).await,
            };
            match app.subscribe_timeline_events(&actor, params).await {
                Ok(source) => {
                    let subscription_id = source.response.subscription_id.clone();
                    send_result(
                        outgoing_tx.clone(),
                        id,
                        serde_json::to_value(&source.response).map_err(|error| {
                            AppServerErrorBody::invalid_request(error.to_string())
                        }),
                    )
                    .await?;
                    let task = spawn_subscription(
                        subscription_id.clone(),
                        source.subscription,
                        outgoing_tx,
                        config.subscription_capacity,
                    );
                    state.subscription_tasks.insert(subscription_id, task);
                    Ok(())
                }
                Err(error) => send_error(outgoing_tx, id, error).await,
            }
        }
        ClientRequestParams::EventUnsubscribe(params) => {
            if !state.initialized_notification_seen {
                return send_error(outgoing_tx, id, AppServerErrorBody::not_initialized()).await;
            }
            if let Err(error) = resolve_request_actor(
                state,
                validator.as_ref(),
                config,
                decoded.method,
                request_actor_credentials,
            )
            .await
            {
                return send_error(outgoing_tx, id, error).await;
            }
            let unsubscribed =
                if let Some(task) = state.subscription_tasks.remove(&params.subscription_id) {
                    task.abort();
                    let _ = task.await;
                    true
                } else {
                    false
                };
            send_result(
                outgoing_tx,
                id,
                serde_json::to_value(EventUnsubscribeResponse {
                    subscription_id: params.subscription_id,
                    unsubscribed,
                })
                .map_err(|error| AppServerErrorBody::invalid_request(error.to_string())),
            )
            .await
        }
        params => {
            let actor = match resolve_request_actor(
                state,
                validator.as_ref(),
                config,
                decoded.method,
                request_actor_credentials,
            )
            .await
            {
                Ok(actor) => actor,
                Err(error) => return send_error(outgoing_tx, id, error).await,
            };
            let request = ClientRequest {
                method: decoded.method,
                params,
            };
            match app.dispatch(&actor, request).await {
                Ok(result) => {
                    if decoded.method == ClientMethod::TurnStart {
                        if let Some(session_id) = result
                            .get("session_id")
                            .and_then(serde_json::Value::as_str)
                            .map(ToString::to_string)
                        {
                            state.owned_turn_sessions.insert(session_id, actor.clone());
                        }
                    }
                    send_result(outgoing_tx, id, Ok(result)).await
                }
                Err(error) => send_error(outgoing_tx, id, error).await,
            }
        }
    }
}

async fn resolve_request_actor(
    state: &RuntimeState,
    validator: &dyn ActorCredentialValidator,
    config: &AppServerConfig,
    method: ClientMethod,
    request_actor_credentials: Option<ActorCredentials>,
) -> Result<Actor, AppServerErrorBody> {
    if !config.auth_enabled {
        return Ok(Actor::regular("anonymous"));
    }

    if matches!(
        method,
        ClientMethod::AuthLoginStart
            | ClientMethod::AuthLoginStatus
            | ClientMethod::AuthLoginCancel
            | ClientMethod::AuthSessionRead
            | ClientMethod::AuthSessionClear
    ) && request_actor_credentials.is_none()
    {
        return state
            .actor
            .clone()
            .ok_or_else(AppServerErrorBody::not_initialized);
    }

    let credentials = request_actor_credentials.ok_or_else(|| {
        AppServerErrorBody::new(
            UNAUTHORIZED,
            "authenticated JSON-RPC request requires actor credentials",
            None,
        )
    })?;
    validator.validate(&credentials).await
}

fn spawn_subscription(
    subscription_id: String,
    mut source: TimelineSubscription,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    _subscription_capacity: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let session_id = source.session_id.clone();
        while let Some(message) = source.events.recv().await {
            match message {
                TimelineSubscriptionMessage::Event(record) => {
                    if send_event(&subscription_id, record, &outgoing_tx)
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                TimelineSubscriptionMessage::Closed(closed) => {
                    let _ = send_subscription_terminated(
                        &subscription_id,
                        &session_id,
                        map_subscription_close_reason(closed.reason),
                        &closed.message,
                        &outgoing_tx,
                    )
                    .await;
                    return;
                }
            }
        }
        let _ = send_subscription_terminated(
            &subscription_id,
            &session_id,
            SubscriptionTerminatedReason::Interrupted,
            "event subscription closed without a close reason",
            &outgoing_tx,
        )
        .await;
    })
}

fn map_subscription_close_reason(
    reason: TimelineSubscriptionCloseReason,
) -> SubscriptionTerminatedReason {
    match reason {
        TimelineSubscriptionCloseReason::Completed => SubscriptionTerminatedReason::Completed,
        TimelineSubscriptionCloseReason::ServerShutdown => {
            SubscriptionTerminatedReason::ServerShutdown
        }
        TimelineSubscriptionCloseReason::Lagged => SubscriptionTerminatedReason::Lagged,
    }
}

async fn send_event(
    subscription_id: &str,
    record: talos_types::talos::TimelineEventEnvelope,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let method = canonical_method(&record.event.event_type);
    let params = serde_json::to_value(TimelineSubscriptionNotification {
        subscription_id: subscription_id.to_string(),
        event: record,
    })
    .map_err(|err| {
        tracing::error!(error = %err, "failed to serialize app-server event");
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            method,
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

async fn send_subscription_terminated(
    subscription_id: &str,
    session_id: &str,
    reason: SubscriptionTerminatedReason,
    message: &str,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let params = serde_json::to_value(SubscriptionTerminatedEnvelope {
        subscription_id: subscription_id.to_string(),
        session_id: session_id.to_string(),
        reason,
        message: message.to_string(),
    })
    .map_err(|err| {
        tracing::error!(error = %err, "failed to serialize app-server subscription termination");
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "subscription.terminated",
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

async fn send_result(
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    id: RequestId,
    result: Result<serde_json::Value, AppServerErrorBody>,
) -> Result<(), ()> {
    match result {
        Ok(result) => outgoing_tx
            .send(OutgoingMessage::Response(JsonRpcResponse::new(id, result)))
            .await
            .map_err(|_| ()),
        Err(error) => send_error(outgoing_tx, id, error).await,
    }
}

async fn send_error(
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    id: RequestId,
    error: AppServerErrorBody,
) -> Result<(), ()> {
    outgoing_tx
        .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
            Some(id),
            error,
        )))
        .await
        .map_err(|_| ())
}

async fn write_loop<W>(mut writer: W, mut rx: mpsc::Receiver<OutgoingMessage>) -> ProtocolResult<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(message) = rx.recv().await {
        let line = match message {
            OutgoingMessage::Response(response) => serde_json::to_string(&response),
            OutgoingMessage::Error(error) => serde_json::to_string(&error),
            OutgoingMessage::Notification(notification) => serde_json::to_string(&notification),
        }
        .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))?;
        writer
            .write_all(line.as_bytes())
            .await
            .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()))?;
    }
    Ok(())
}

async fn cancel_owned_turns(
    app: Arc<dyn AppServerApplication>,
    session_actors: &HashMap<String, Actor>,
) {
    for (session_id, session_actor) in session_actors {
        if let Err(error) = app.cancel_active_turn(session_actor, session_id).await {
            tracing::warn!(session_id, error = ?error, "failed to cancel owned turn");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;
    use talos_app_protocol::errors::{NOT_INITIALIZED, PARSE_ERROR, UNAUTHORIZED};
    use talos_app_protocol::requests::ActorCredentials;
    use talos_service::{
        Actor, EventReplay, TimelineSubscriptionCloseReason, TimelineSubscriptionClosed,
        TimelineSubscriptionMessage,
    };
    use talos_types::talos::{
        TalosTimelineEvent, TalosTimelineEventPayload, TalosTimelineEventType,
        TalosTurnStartedEvent, TimelineEventEnvelope,
    };
    use talos_types::SubscribeTimelineRequest;
    use tokio::io::{duplex, split, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::sync::{broadcast, mpsc};

    use super::*;
    use crate::application::SubscriptionSource;
    use crate::auth::ActorCredentialValidator;

    #[derive(Default)]
    struct MockApp {
        dispatched: Mutex<Vec<String>>,
        dispatched_actors: Mutex<Vec<String>>,
        cancelled: Mutex<Vec<String>>,
        cancelled_actors: Mutex<Vec<String>>,
        drained: Mutex<usize>,
        live_tx: Mutex<Option<broadcast::Sender<TimelineEventEnvelope>>>,
        stream_txs: Mutex<Vec<mpsc::Sender<TimelineSubscriptionMessage>>>,
    }

    #[async_trait]
    impl AppServerApplication for MockApp {
        async fn dispatch(
            &self,
            actor: &Actor,
            request: talos_app_protocol::requests::ClientRequest,
        ) -> ProtocolResult<serde_json::Value> {
            self.dispatched
                .lock()
                .expect("dispatch lock")
                .push(request.method.as_str().to_string());
            self.dispatched_actors
                .lock()
                .expect("dispatch actor lock")
                .push(actor.user_id.clone());
            Ok(match request.params {
                ClientRequestParams::TurnStart(params) => json!({
                    "request_id": "req-1",
                    "message_id": "msg-1",
                    "session_id": params.session_id,
                    "turn_id": "turn-1",
                    "status": "accepted",
                }),
                _ => json!({ "ok": true }),
            })
        }

        async fn subscribe_timeline_events(
            &self,
            _actor: &Actor,
            params: SubscribeTimelineRequest,
        ) -> ProtocolResult<SubscriptionSource> {
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
                    Ok(SubscriptionSource {
                        response: talos_types::SubscribeTimelineResponse {
                            subscription_id,
                            session_id: params.session_id.clone(),
                            high_water_mark_event_id: None,
                        },
                        subscription: talos_service::TimelineSubscription {
                            session_id: params.session_id,
                            high_water_mark_event_id: None,
                            replay: EventReplay {
                                events: Vec::new(),
                                high_water_mark_event_id: None,
                                last_replay_seq: 0,
                                cursor_status: talos_service::TimelineCursorStatus::FromBeginning,
                            },
                            events,
                            has_live_turn: false,
                        },
                    })
                }
                None => Err(AppServerErrorBody::method_not_found("event.subscribe")),
            }
        }

        async fn cancel_active_turn(&self, actor: &Actor, session_id: &str) -> ProtocolResult<()> {
            self.cancelled
                .lock()
                .expect("cancel lock")
                .push(session_id.to_string());
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
        async fn validate(&self, _credentials: &ActorCredentials) -> ProtocolResult<Actor> {
            Err(AppServerErrorBody::from_talos_error(
                talos_error::TalosError::AuthError("invalid token".to_string()),
            ))
        }
    }

    struct ExistingTokenValidator;

    #[async_trait]
    impl ActorCredentialValidator for ExistingTokenValidator {
        async fn validate(&self, credentials: &ActorCredentials) -> ProtocolResult<Actor> {
            match &credentials.credential {
                talos_app_protocol::requests::ActorCredential::ExistingSessionToken { token }
                    if token == "user-token" =>
                {
                    Ok(Actor::regular("user-1"))
                }
                _ => Err(AppServerErrorBody::from_talos_error(
                    talos_error::TalosError::AuthError("invalid token".to_string()),
                )),
            }
        }
    }

    #[tokio::test]
    async fn request_before_initialize_returns_not_initialized() {
        let output = run_with_input(session_read_request()).await;
        let messages = parse_output(&output);

        assert_eq!(messages[0]["error"]["code"], NOT_INITIALIZED);
    }

    #[tokio::test]
    async fn malformed_line_returns_parse_error_and_keeps_running() {
        let output = run_with_input(format!("{{\n{}", initialize_request("secret"))).await;
        let messages = parse_output(&output);

        assert_eq!(messages[0]["error"]["code"], PARSE_ERROR);
        assert_eq!(messages[1]["result"]["protocol_version"]["major"], 0);
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

        assert_eq!(messages[1]["error"]["code"], UNAUTHORIZED);
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

        assert!(messages[1]["result"]["ok"].as_bool().unwrap_or(false));
        assert_eq!(
            app.dispatched.lock().expect("dispatch lock").as_slice(),
            &["session.get"]
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

        assert!(messages[1]["result"]["ok"].as_bool().unwrap_or(false));
        assert_eq!(
            app.dispatched_actors
                .lock()
                .expect("dispatch actor lock")
                .as_slice(),
            &["anonymous"]
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

        assert_eq!(messages[1]["error"]["code"], UNAUTHORIZED);
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

        assert!(messages[1]["result"]["ok"].as_bool().unwrap_or(false));
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

        assert!(messages[1]["result"]["ok"].as_bool().unwrap_or(false));
        assert_eq!(
            app.dispatched.lock().expect("dispatch lock").as_slice(),
            &["session.get"]
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

        assert_eq!(messages[1]["result"]["session_id"], "s1");
        assert_eq!(
            app.cancelled.lock().expect("cancel lock").as_slice(),
            &["s1"]
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

        assert_eq!(messages[1]["result"]["session_id"], "s1");
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
        assert_eq!(initialize["result"]["protocol_version"]["major"], 0);
        assert_eq!(first_subscribe["result"]["session_id"], "s1");
        assert_eq!(second_subscribe["result"]["session_id"], "s1");

        let first_subscription_id = first_subscribe["result"]["subscription_id"]
            .as_str()
            .expect("first subscription id")
            .to_string();
        let second_subscription_id = second_subscribe["result"]["subscription_id"]
            .as_str()
            .expect("second subscription id")
            .to_string();
        assert_ne!(first_subscription_id, second_subscription_id);

        let senders = app.stream_txs.lock().expect("stream tx lock").clone();
        assert_eq!(senders.len(), 2);
        senders[0]
            .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                "ev_1", 1,
            )))
            .await
            .expect("send first subscription event");
        senders[1]
            .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                "ev_2", 2,
            )))
            .await
            .expect("send second subscription event");

        let first_notification = read_output_line(&mut output).await;
        let second_notification = read_output_line(&mut output).await;
        assert_eq!(
            first_notification["params"]["subscription_id"],
            first_subscription_id
        );
        assert_eq!(first_notification["params"]["event"]["event_id"], "ev_1");
        assert_eq!(
            second_notification["params"]["subscription_id"],
            second_subscription_id
        );
        assert_eq!(second_notification["params"]["event"]["event_id"], "ev_2");

        client_write
            .write_all(event_unsubscribe_request(4, &first_subscription_id).as_bytes())
            .await
            .expect("write unsubscribe request");
        let unsubscribe = read_output_line(&mut output).await;
        assert_eq!(
            unsubscribe["result"]["subscription_id"],
            first_subscription_id
        );
        assert!(unsubscribe["result"]["unsubscribed"]
            .as_bool()
            .unwrap_or(false));

        assert!(senders[0]
            .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                "ev_3", 3
            )))
            .await
            .is_err());
        senders[1]
            .send(TimelineSubscriptionMessage::Event(timeline_envelope(
                "ev_4", 4,
            )))
            .await
            .expect("second subscription should remain active");
        let remaining_notification = read_output_line(&mut output).await;
        assert_eq!(
            remaining_notification["params"]["subscription_id"],
            second_subscription_id
        );
        assert_eq!(
            remaining_notification["params"]["event"]["event_id"],
            "ev_4"
        );

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
        let subscription_id = subscribe["result"]["subscription_id"]
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
        assert_eq!(notification["method"], "subscription.terminated");
        assert_eq!(notification["params"]["subscription_id"], subscription_id);
        assert_eq!(notification["params"]["session_id"], "s1");
        assert_eq!(notification["params"]["reason"], "completed");

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
        let subscription_id = subscribe["result"]["subscription_id"]
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
        assert_eq!(notification["method"], "subscription.terminated");
        assert_eq!(notification["params"]["subscription_id"], subscription_id);
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
        let (mut client, server) = duplex(16 * 1024);
        let (server_read, server_write) = split(server);
        let server_task =
            tokio::spawn(run_jsonl(server_read, server_write, app, validator, config));
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
        format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "client_info": {
                        "name": "test",
                        "version": "0.1.2"
                    },
                    "capabilities": {
                        "protocol_version": { "major": 0, "minor": 2 },
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
                "method": "session.get",
                "params": { "session_id": "s1" }
            })
        )
    }

    fn session_read_request_with_actor_token(token: &str) -> String {
        format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "session.get",
                "actor_credentials": {
                    "credential": {
                        "type": "existing_session_token",
                        "token": token
                    }
                },
                "params": { "session_id": "s1" }
            })
        )
    }

    fn auth_session_read_request() -> String {
        format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "auth.session.read",
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
                "method": "turn.start",
                "params": {
                    "session_id": "s1",
                    "input": [{ "type": "text", "text": "hello" }],
                    "workspace_id": null,
                    "client_request_id": "request-1"
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
                "method": "event.subscribe",
                "params": {
                    "session_id": "s1",
                    "since_event_id": null,
                    "replay_limit": 1
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
                "method": "event.unsubscribe",
                "params": {
                    "subscription_id": subscription_id
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

    fn timeline_envelope(event_id: &str, event_seq: i64) -> TimelineEventEnvelope {
        let event = TalosTimelineEvent {
            event_seq: Some(event_seq),
            event_id: Some(event_id.to_string()),
            created_at_ms: Some(100 + event_seq),
            event_type: TalosTimelineEventType::TurnStarted,
            turn_id: "turn-1".to_string(),
            message_id: "msg-1".to_string(),
            session_id: "s1".to_string(),
            sequence: event_seq as u64,
            payload: TalosTimelineEventPayload::TurnStarted(TalosTurnStartedEvent {
                request_id: "req-1".to_string(),
                turn_id: "turn-1".to_string(),
                started_at_ms: 100 + event_seq as u64,
            }),
        };
        TimelineEventEnvelope {
            event_id: event_id.to_string(),
            event_seq,
            created_at_ms: 100 + event_seq,
            session_id: event.session_id.clone(),
            turn_id: event.turn_id.clone(),
            message_id: event.message_id.clone(),
            event,
        }
    }

    fn parse_output(output: &str) -> Vec<serde_json::Value> {
        output
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }
}
