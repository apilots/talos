use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use talos_app_protocol::errors::{AppServerErrorBody, ALREADY_INITIALIZED, UNAUTHORIZED};
use talos_app_protocol::events::{
    canonical_method, SubscriptionTerminatedEnvelope, SubscriptionTerminatedReason,
};
use talos_app_protocol::jsonrpc::{
    parse_message_line, JsonRpcErrorResponse, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use talos_app_protocol::requests::{
    ActorCredentials, ClientMethod, ClientRequest, ClientRequestParams, EventSubscribeResponse,
    EventUnsubscribeResponse, InitializeResponse, ServerInfo,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_application_service::Actor;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::application::{AppServerApplication, ProtocolResult};
use crate::auth::ActorCredentialValidator;
use crate::events::notification_envelope;

#[derive(Debug, Clone)]
pub struct AppServerConfig {
    pub outgoing_capacity: usize,
    pub subscription_capacity: usize,
    pub server_name: String,
    pub server_version: String,
    pub auth_enabled: bool,
}

impl Default for AppServerConfig {
    fn default() -> Self {
        Self {
            outgoing_capacity: 256,
            subscription_capacity: 256,
            server_name: "talos-app-server".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            auth_enabled: false,
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

    cancel_owned_turns(
        app.clone(),
        state.actor.as_ref(),
        &state.owned_turn_sessions,
    )
    .await;

    const DRAIN_BACKGROUND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    if let Err(err) = app.drain_background_tasks(DRAIN_BACKGROUND_TIMEOUT).await {
        tracing::warn!(error = ?err, "drain_background_tasks failed during shutdown");
    }

    // Keep subscriptions alive while background drains run so final turn events can still be
    // forwarded. After the drain boundary, abort them explicitly; idle subscriptions block on
    // live_events.recv() and would otherwise keep stdio shutdown from completing.
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
    owned_turn_sessions: HashSet<String>,
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
        ClientRequestParams::Initialize(params) => {
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
            match validator.validate(&params.actor_credentials).await {
                Ok(actor) => {
                    state.actor = Some(actor);
                    let result = serde_json::to_value(InitializeResponse {
                        protocol_version: ProtocolVersion::CURRENT,
                        server_info: ServerInfo {
                            name: config.server_name.clone(),
                            version: config.server_version.clone(),
                            auth_enabled: config.auth_enabled,
                        },
                    })
                    .map_err(|error| AppServerErrorBody::invalid_request(error.to_string()));
                    send_result(outgoing_tx, id, result).await
                }
                Err(error) => send_error(outgoing_tx, id, error).await,
            }
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
            match app.subscribe_session_events(&actor, params).await {
                Ok(source) => {
                    let subscription_id = format!("sub_{}", Uuid::new_v4().simple());
                    let response = EventSubscribeResponse {
                        subscription_id: subscription_id.clone(),
                        session_id: source.session_id.clone(),
                        high_water_mark_event_id: source
                            .high_water_mark_event_id
                            .as_ref()
                            .map(|value| value.to_string()),
                    };
                    send_result(
                        outgoing_tx.clone(),
                        id,
                        serde_json::to_value(response).map_err(|error| {
                            AppServerErrorBody::invalid_request(error.to_string())
                        }),
                    )
                    .await?;
                    let task = spawn_subscription(
                        subscription_id.clone(),
                        source,
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
                            state.owned_turn_sessions.insert(session_id);
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
    if !config.auth_enabled || is_public_auth_method(method) {
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

fn is_public_auth_method(method: ClientMethod) -> bool {
    matches!(method, ClientMethod::AuthLogin | ClientMethod::AuthRegister)
}

fn spawn_subscription(
    subscription_id: String,
    mut source: crate::application::SubscriptionSource,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    _subscription_capacity: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_sent_seq = source.replay.last_replay_seq;
        for record in source.replay.events {
            last_sent_seq = record.event_seq;
            if send_event(&subscription_id, record, &outgoing_tx)
                .await
                .is_err()
            {
                return;
            }
        }

        loop {
            match source.live_events.recv().await {
                Ok(record) => {
                    if record.session_id != source.session_id || record.event_seq <= last_sent_seq {
                        continue;
                    }
                    last_sent_seq = record.event_seq;
                    let terminal = is_terminal_event(&record.payload);
                    if send_event(&subscription_id, record, &outgoing_tx)
                        .await
                        .is_err()
                    {
                        return;
                    }
                    if terminal {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "app-server subscription lagged");
                    let _ = send_subscription_terminated(
                        &subscription_id,
                        &source.session_id,
                        SubscriptionTerminatedReason::SlowConsumer,
                        "subscription lagged behind retained live events",
                        &outgoing_tx,
                    )
                    .await;
                    return;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    })
}

async fn send_event(
    subscription_id: &str,
    record: talos_session_store::SessionEventRecord,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let envelope = notification_envelope(subscription_id, record);
    let method = canonical_method(&envelope.event.event_type);
    let params = serde_json::to_value(envelope).map_err(|err| {
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
    let envelope = SubscriptionTerminatedEnvelope {
        subscription_id: subscription_id.to_string(),
        session_id: session_id.to_string(),
        reason,
        message: message.to_string(),
    };
    let params = serde_json::to_value(envelope).map_err(|err| {
        tracing::error!(error = %err, "failed to serialize subscription termination");
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "subscription.terminated",
            Some(params),
        )))
        .await
        .map_err(|_| ())
}

fn is_terminal_event(payload: &talos_types::talos::TalosEventPayload) -> bool {
    matches!(
        payload,
        talos_types::talos::TalosEventPayload::MessageCompleted(_)
            | talos_types::talos::TalosEventPayload::MessageError(_)
    )
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
    actor: Option<&Actor>,
    session_ids: &HashSet<String>,
) {
    let Some(actor) = actor else {
        return;
    };
    for session_id in session_ids {
        if let Err(error) = app.cancel_active_turn(actor, session_id).await {
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
    use talos_app_protocol::requests::{ActorCredentials, EventSubscribeParams};
    use talos_application_service::{Actor, EventReplay};
    use talos_session_store::SessionEventRecord;
    use tokio::io::{duplex, split, AsyncReadExt, AsyncWriteExt};
    use tokio::sync::broadcast;

    use super::*;
    use crate::application::SubscriptionSource;
    use crate::auth::{ActorCredentialValidator, EnvLaunchTokenValidator};

    #[derive(Default)]
    struct MockApp {
        dispatched: Mutex<Vec<String>>,
        cancelled: Mutex<Vec<String>>,
        drained: Mutex<usize>,
        live_tx: Mutex<Option<broadcast::Sender<SessionEventRecord>>>,
    }

    #[async_trait]
    impl AppServerApplication for MockApp {
        async fn dispatch(
            &self,
            _actor: &Actor,
            request: talos_app_protocol::requests::ClientRequest,
        ) -> ProtocolResult<serde_json::Value> {
            self.dispatched
                .lock()
                .expect("dispatch lock")
                .push(request.method.as_str().to_string());
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

        async fn subscribe_session_events(
            &self,
            _actor: &Actor,
            params: EventSubscribeParams,
        ) -> ProtocolResult<SubscriptionSource> {
            let live_tx = self.live_tx.lock().expect("live tx lock").as_ref().cloned();
            match live_tx {
                Some(live_tx) => Ok(SubscriptionSource {
                    session_id: params.session_id,
                    high_water_mark_event_id: None,
                    replay: EventReplay {
                        events: Vec::new(),
                        last_replay_seq: 0,
                    },
                    live_events: live_tx.subscribe(),
                    has_live_turn: false,
                }),
                None => Err(AppServerErrorBody::method_not_found("event.subscribe")),
            }
        }

        async fn cancel_active_turn(&self, _actor: &Actor, session_id: &str) -> ProtocolResult<()> {
            self.cancelled
                .lock()
                .expect("cancel lock")
                .push(session_id.to_string());
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
                talos_app_protocol::requests::ActorCredential::LocalLaunchToken { token }
                    if token == "secret" =>
                {
                    Ok(Actor::regular("local-desktop"))
                }
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
        let output = run_with_app_and_validator(
            initialize_request("bad"),
            Arc::new(MockApp::default()),
            Arc::new(RejectingValidator),
        )
        .await;
        let messages = parse_output(&output);

        assert_eq!(messages[0]["error"]["code"], UNAUTHORIZED);
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
            &["session.read"]
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
            &["session.read"]
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

    async fn run_with_input(input: String) -> String {
        run_with_app(input, Arc::new(MockApp::default())).await
    }

    async fn run_with_app(input: String, app: Arc<MockApp>) -> String {
        run_with_app_and_validator(
            input,
            app,
            Arc::new(EnvLaunchTokenValidator::new("secret", "tester")),
        )
        .await
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
        let server_task = tokio::spawn(run_jsonl(server_read, server_write, app, validator, config));
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

    fn initialize_request(token: &str) -> String {
        format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "client_info": {
                        "name": "test",
                        "version": "0.1.0"
                    },
                    "capabilities": {
                        "protocol_version": { "major": 0, "minor": 2 },
                        "supports_server_requests": false,
                        "supports_event_replay": true
                    },
                    "actor_credentials": {
                        "credential": {
                            "type": "local_launch_token",
                            "token": token
                        }
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
                "method": "session.read",
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
                "method": "session.read",
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
        format!(
            "{}\n",
            json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "event.subscribe",
                "params": {
                    "session_id": "s1",
                    "since_event_id": null,
                    "replay_limit": 1
                }
            })
        )
    }

    fn parse_output(output: &str) -> Vec<serde_json::Value> {
        output
            .lines()
            .map(|line| serde_json::from_str(line).expect("json line"))
            .collect()
    }
}
