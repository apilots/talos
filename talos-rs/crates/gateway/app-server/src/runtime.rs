use std::collections::HashMap;
use std::sync::Arc;

use talos_app_protocol::errors::{AppServerErrorBody, ALREADY_INITIALIZED};
use talos_app_protocol::events::{
    timeline_event_method, SubscriptionTerminatedNotification, SubscriptionTerminatedReason,
    TimelineEventNotification,
};
use talos_app_protocol::jsonrpc::{
    parse_message_line, JsonRpcErrorResponse, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use talos_app_protocol::requests::{
    ActorCredentials, ClientKind, ClientMethod, ClientRequest, ClientRequestParams,
    InitializeResponse, ServerInfo,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_error::{ApiError, ErrorCode};
use talos_service::{
    Actor, TimelineSubscription, TimelineSubscriptionCloseReason, TimelineSubscriptionMessage,
};
use talos_types::{ApiResponse, EventUnsubscribeResponse};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::application::{AppServerApplication, ProtocolResult};
use crate::auth::ActorCredentialValidator;

const DEFAULT_MAX_INCOMING_FRAME_BYTES: usize = 68 * 1024 * 1024;

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
    run_stdio_with_max_frame_bytes(app, validator, config, DEFAULT_MAX_INCOMING_FRAME_BYTES).await
}

/// Runs the stdio transport with an explicit JSONL frame limit.
pub async fn run_stdio_with_max_frame_bytes(
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
    max_incoming_frame_bytes: usize,
) -> ProtocolResult<()> {
    run_jsonl_with_max_frame_bytes(
        tokio::io::stdin(),
        tokio::io::stdout(),
        app,
        validator,
        config,
        max_incoming_frame_bytes,
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
    run_jsonl_with_max_frame_bytes(
        reader,
        writer,
        app,
        validator,
        config,
        DEFAULT_MAX_INCOMING_FRAME_BYTES,
    )
    .await
}

/// Runs one JSONL transport with an explicit maximum request-frame size.
pub async fn run_jsonl_with_max_frame_bytes<R, W>(
    reader: R,
    writer: W,
    app: Arc<dyn AppServerApplication>,
    validator: Arc<dyn ActorCredentialValidator>,
    config: AppServerConfig,
    max_incoming_frame_bytes: usize,
) -> ProtocolResult<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let max_incoming_frame_bytes = max_incoming_frame_bytes.max(1);
    let (outgoing_tx, outgoing_rx) = mpsc::channel(config.outgoing_capacity.max(1));
    let mut writer_task = tokio::spawn(write_loop(writer, outgoing_rx));
    let mut state = RuntimeState::default();
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    let mut writer_finished = false;

    loop {
        tokio::select! {
            line_result = read_bounded_line(
                &mut reader,
                &mut line,
                max_incoming_frame_bytes,
            ) => {
                match line_result {
                    Ok(BoundedLineRead::Line) => {
                        let line = match std::str::from_utf8(&line) {
                            Ok(line) => line,
                            Err(error) => {
                                let _ = outgoing_tx
                                    .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                                        None,
                                        AppServerErrorBody::parse_error(error.to_string()),
                                    )))
                                    .await;
                                break;
                            }
                        };
                        if handle_line(
                            line,
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
                    Ok(BoundedLineRead::Eof) => break,
                    Ok(BoundedLineRead::LimitExceeded) => {
                        let _ = outgoing_tx
                            .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                                None,
                                AppServerErrorBody::parse_error(format!(
                                    "incoming JSONL frame exceeds {} bytes",
                                    max_incoming_frame_bytes
                                )),
                            )))
                            .await;
                        break;
                    }
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

enum BoundedLineRead {
    Eof,
    Line,
    LimitExceeded,
}

async fn read_bounded_line<R>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max_bytes: usize,
) -> std::io::Result<BoundedLineRead>
where
    R: AsyncBufRead + Unpin,
{
    line.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                BoundedLineRead::Eof
            } else {
                BoundedLineRead::Line
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |position| position + 1);
        if line.len().saturating_add(consumed) > max_bytes {
            return Ok(BoundedLineRead::LimitExceeded);
        }
        line.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(BoundedLineRead::Line);
        }
    }
}

#[derive(Default)]
struct RuntimeState {
    actor: Option<Actor>,
    session_source: Option<talos_types::talos::SessionSource>,
    initialized_notification_seen: bool,
    owned_turn_sessions: HashMap<String, OwnedTurn>,
    subscription_tasks: HashMap<String, tokio::task::JoinHandle<()>>,
}

struct OwnedTurn {
    actor: Actor,
    turn_id: String,
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
                    ),
                )
                .await;
            }
            state.actor = Some(Actor::regular("anonymous"));
            state.session_source = Some(match params.client_info.kind {
                ClientKind::Desktop => talos_types::talos::SessionSource::desktop(),
                ClientKind::Cli => talos_types::talos::SessionSource::cli(),
                ClientKind::Sdk => talos_types::talos::SessionSource::sdk(),
            });
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
                Err(error) => return send_application_error(outgoing_tx, id, error).await,
            };
            match app.subscribe_timeline_events(&actor, params).await {
                Ok(subscription) => {
                    let response = subscription.response();
                    let subscription_id = response.subscription_id.clone();
                    send_result(
                        outgoing_tx.clone(),
                        id,
                        serde_json::to_value(&response).map_err(|error| {
                            AppServerErrorBody::invalid_request(error.to_string())
                        }),
                    )
                    .await?;
                    let task = spawn_subscription(
                        subscription_id.clone(),
                        subscription,
                        outgoing_tx,
                        config.subscription_capacity,
                    );
                    state.subscription_tasks.insert(subscription_id, task);
                    Ok(())
                }
                Err(error) => send_application_error(outgoing_tx, id, error).await,
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
                return send_application_error(outgoing_tx, id, error).await;
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
                Err(error) => return send_application_error(outgoing_tx, id, error).await,
            };
            let result = match params {
                ClientRequestParams::TurnStart(request) => {
                    let source = state
                        .session_source
                        .clone()
                        .expect("initialized app-server must have a Session source");
                    app.start_turn(&actor, source, request).await
                }
                params => {
                    app.dispatch(
                        &actor,
                        ClientRequest {
                            method: decoded.method,
                            params,
                        },
                    )
                    .await
                }
            };
            match result {
                Ok(result) => {
                    if let Some(identity) = result.started_turn {
                        state.owned_turn_sessions.insert(
                            identity.session_id,
                            OwnedTurn {
                                actor: actor.clone(),
                                turn_id: identity.turn_id,
                            },
                        );
                    }
                    send_result(outgoing_tx, id, Ok(result.body)).await
                }
                Err(error) => send_application_error(outgoing_tx, id, error).await,
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
) -> Result<Actor, ApiError> {
    if !config.auth_enabled {
        return Ok(Actor::auth_disabled());
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
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated));
    }

    let credentials =
        request_actor_credentials.ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
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
                    if send_event(&subscription_id, *record, &outgoing_tx)
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                TimelineSubscriptionMessage::MessageQueueChanged(change) => {
                    if send_message_queue_changed(change, &outgoing_tx)
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

async fn send_message_queue_changed(
    change: talos_types::MessageQueueChangedNotification,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let params = serde_json::to_value(change).map_err(|err| {
        tracing::error!(error = %err, "failed to serialize message queue change");
    })?;
    outgoing_tx
        .send(OutgoingMessage::Notification(JsonRpcNotification::new(
            "messageQueue/changed",
            Some(params),
        )))
        .await
        .map_err(|_| ())
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
    record: talos_agent_api::TimelineEvent,
    outgoing_tx: &mpsc::Sender<OutgoingMessage>,
) -> Result<(), ()> {
    let method = timeline_event_method(&record.event.payload);
    let params = serde_json::to_value(TimelineEventNotification {
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
    let params = serde_json::to_value(SubscriptionTerminatedNotification {
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
            "subscription/terminated",
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
            .send(OutgoingMessage::Response(JsonRpcResponse::new(
                id,
                api_success_value(result)?,
            )))
            .await
            .map_err(|_| ()),
        Err(error) => send_error(outgoing_tx, id, error).await,
    }
}

async fn send_application_error(
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    id: RequestId,
    error: ApiError,
) -> Result<(), ()> {
    let result = api_error_value(error).map_err(|error| {
        tracing::error!(error = %error, "failed to serialize app-server application error");
    })?;
    outgoing_tx
        .send(OutgoingMessage::Response(JsonRpcResponse::new(id, result)))
        .await
        .map_err(|_| ())
}

fn api_success_value(result: serde_json::Value) -> Result<serde_json::Value, ()> {
    serde_json::to_value(ApiResponse::success(result)).map_err(|error| {
        tracing::error!(error = %error, "failed to serialize app-server API success");
    })
}

fn api_error_value(error: ApiError) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(ApiResponse::<serde_json::Value>::error(error))
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
    owned_turns: &HashMap<String, OwnedTurn>,
) {
    for (session_id, owned_turn) in owned_turns {
        if let Err(error) = app
            .cancel_active_turn(&owned_turn.actor, session_id, &owned_turn.turn_id)
            .await
        {
            tracing::warn!(session_id, error = ?error, "failed to cancel owned turn");
        }
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
