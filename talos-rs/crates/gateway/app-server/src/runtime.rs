mod desktop_host;
mod local_requests;
mod subscriptions;
use desktop_host::{complete_host_error, complete_host_response};

mod notifications;
use notifications::{
    map_subscription_close_reason, send_active_turn_changed, send_event,
    send_message_queue_changed, send_subscription_terminated,
};

use std::collections::HashMap;
use std::sync::Arc;

use talos_app_protocol::errors::{AppServerErrorBody, AppServerErrorCode, ALREADY_INITIALIZED};
use talos_app_protocol::events::SubscriptionTerminatedReason;
use talos_app_protocol::jsonrpc::{
    parse_message_line, JsonRpcErrorResponse, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, RequestId,
};
use talos_app_protocol::requests::{
    ActorCredentials, ClientKind, ClientMethod, ClientRequest, ClientRequestParams,
    InitializeResponse, ServerInfo,
};
use talos_app_protocol::version::ProtocolVersion;
use talos_error::{ApiError, ErrorCode, TalosError, TalosResult};
use talos_logging::report_error;
use talos_service::{Actor, TimelineSubscription, TimelineSubscriptionMessage};
use talos_types::ApiResponse;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::application::{AppServerApplication, ProtocolResult};
use crate::auth::ActorCredentialValidator;

const DEFAULT_MAX_INCOMING_FRAME_BYTES: usize = 68 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct AppServerConfig {
    pub desktop_host: Option<Arc<crate::DesktopHostBridge>>,
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
            desktop_host: None,
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
    Request(JsonRpcRequest),
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
    let browser_requests = config
        .desktop_host
        .as_ref()
        .map(|host| host.take_receiver())
        .transpose()
        .map_err(|error| {
            AppServerErrorBody::new(
                AppServerErrorCode::InternalError,
                error.to_api_error().message,
            )
        })?;
    // Host responses must progress while a serialized business command awaits them.
    // Worker commands remain serial; retained queries and bounded deletion progress independently.
    let subscriptions = Arc::new(subscriptions::Subscriptions::new(
        config.subscription_capacity,
    ));
    let local_requests = Arc::new(local_requests::LocalRequests::new(
        app.clone(),
        validator.clone(),
        config.auth_enabled,
        subscriptions.clone(),
    ));
    let stop = tokio_util::sync::CancellationToken::new();
    let (line_tx, mut line_rx) = mpsc::channel(16);
    let read_loop = desktop_host::read_requests(
        reader,
        max_incoming_frame_bytes,
        config.desktop_host.clone(),
        outgoing_tx.clone(),
        line_tx,
        stop.clone(),
        local_requests.clone(),
    );
    let host_loop =
        desktop_host::forward_requests(browser_requests, outgoing_tx.clone(), stop.clone());
    let serve_loop = async {
        let mut state = RuntimeState {
            actor: None,
            session_source: None,
            initialized_notification_seen: false,
            owned_turn_sessions: HashMap::new(),
            subscriptions: subscriptions.clone(),
        };
        let mut line = Vec::new();
        let mut _frame_permit = None;
        let mut writer_finished = false;
        let mut worker_failures = app.subscribe_worker_failures();

        loop {
            tokio::select! {
                changed = async {
                    match &mut worker_failures {
                        Some(receiver) => receiver.changed().await,
                        None => std::future::pending().await,
                    }
                } => {
                    if changed.is_err() {
                        worker_failures = None;
                    } else if let Some(error) = worker_failures.as_ref().and_then(|receiver| receiver.borrow().clone()) {
                        let params = match serde_json::to_value(error) {
                            Ok(params) => params,
                            Err(source) => {
                                report_error(&TalosError::internal_with_source("Serialize Worker registration failure.", source));
                                break;
                            }
                        };
                        if outgoing_tx.send(OutgoingMessage::Notification(JsonRpcNotification::new(
                            "desktop/worker/failed", Some(params),
                        ))).await.is_err() { break; }
                    } else if outgoing_tx.send(OutgoingMessage::Notification(JsonRpcNotification::new(
                        "desktop/worker/ready", None,
                    ))).await.is_err() { break; }
                }

                line_result = async {match line_rx.recv().await {Some((result,frame,permit))=>{line=frame;_frame_permit=permit;result},None=>Ok(BoundedLineRead::Eof)}} => {
                    match line_result {
                        Ok(BoundedLineRead::Line) => {
                            let line = match std::str::from_utf8(&line) {
                                Ok(line) => line,
                                Err(error) => {
                                    let error = TalosError::from_source(
                                        ErrorCode::InvalidRequest,
                                        "The app-server request is not valid UTF-8.",
                                        error,
                                    );
                                    report_error(&error);
                                    // A disconnected receiver is already handled by the output task.
                                    let _ = outgoing_tx
                                        .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                                            None,
                                            AppServerErrorBody::parse_error(error.to_api_error().message),
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
                            if let Some(actor) = &state.actor { local_requests.initialized(actor); }
                        }
                        Ok(BoundedLineRead::Eof) => break,
                        Ok(BoundedLineRead::LimitExceeded) => {
                            // A disconnected receiver is already handled by the output task.
                            let _ = send_error(
                                outgoing_tx.clone(),
                                /*id*/ None,
                                AppServerErrorBody::parse_error(format!(
                                    "incoming JSONL frame exceeds {max_incoming_frame_bytes} bytes"
                                )),
                            ).await;
                            break;
                        }
                        Err(err) => {
                            let error = TalosError::internal_with_source(
                                "Failed to read the app-server request stream.",
                                err,
                            );
                            report_error(&error);
                            // A disconnected receiver is already handled by the output task.
                            let _ = outgoing_tx
                                .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(
                                    None,
                                    AppServerErrorBody::parse_error(error.to_api_error().message),
                                )))
                                .await;
                            break;
                        }
                    }
                    line=Vec::new();
                    _frame_permit=None;
                }
                writer_result = &mut writer_task => {
                    match writer_result {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => report_error(&err),
                        Err(err) => report_error(&TalosError::internal_with_source("The app-server output task stopped unexpectedly.", err)),
                    }
                    writer_finished = true;
                    break;
                }
            }
        }

        if let Some(host) = &config.desktop_host {
            host.close();
        }
        cancel_owned_turns(app.clone(), &state.owned_turn_sessions).await;

        const DRAIN_BACKGROUND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

        if let Err(err) = app.drain_background_tasks(DRAIN_BACKGROUND_TIMEOUT).await {
            report_error(&err.context("drain background tasks during shutdown"));
        }

        // Keep subscriptions alive while background drains run so final turn events can still be
        // forwarded. After the drain boundary, abort them explicitly; idle subscriptions wait on
        // the TalosService stream and would otherwise keep stdio shutdown from completing.
        subscriptions.shutdown().await;

        stop.cancel();
        drop(outgoing_tx);

        if !writer_finished {
            match writer_task.await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => report_error(&err),
                Err(err) => report_error(&TalosError::internal_with_source(
                    "The app-server output task stopped unexpectedly.",
                    err,
                )),
            }
        }
        Ok(())
    };
    let (_, _, result) = tokio::join!(read_loop, host_loop, serve_loop);
    result
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

struct RuntimeState {
    actor: Option<Actor>,
    session_source: Option<talos_types::talos::SessionSource>,
    initialized_notification_seen: bool,
    owned_turn_sessions: HashMap<String, OwnedTurn>,
    subscriptions: Arc<subscriptions::Subscriptions>,
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
            return send_error(outgoing_tx, /*id*/ None, error).await;
        }
    };

    match message {
        JsonRpcMessage::Request(request) => {
            handle_request(request, state, app, validator, config, outgoing_tx).await
        }
        JsonRpcMessage::Notification(notification) => {
            if notification.method == "initialized" {
                state.initialized_notification_seen = true;
                if state.actor.is_some() {
                    if let Some(host) = &config.desktop_host {
                        host.activate();
                    }
                }
            }
            Ok(())
        }
        JsonRpcMessage::Response(response) => {
            if let Some(host) = &config.desktop_host {
                complete_host_response(host, response);
            }
            Ok(())
        }
        JsonRpcMessage::Error(response) => {
            if let Some(host) = &config.desktop_host {
                complete_host_error(host, response);
            }
            Ok(())
        }
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
        .map_err(|error| send_error(outgoing_tx.clone(), Some(id.clone()), error));
    let decoded = match decoded {
        Ok(decoded) => decoded,
        Err(send) => return send.await,
    };

    if decoded.method != ClientMethod::Initialize && state.actor.is_none() {
        return send_error(outgoing_tx, Some(id), AppServerErrorBody::not_initialized()).await;
    }

    match decoded.params {
        ClientRequestParams::Initialize(params) => {
            if state.actor.is_some() {
                return send_error(
                    outgoing_tx,
                    Some(id),
                    AppServerErrorBody::new(
                        ALREADY_INITIALIZED,
                        "app-server is already initialized",
                    ),
                )
                .await;
            }
            if config.desktop_host.is_some() && params.client_info.kind != ClientKind::Desktop {
                return send_error(
                    outgoing_tx,
                    Some(id),
                    AppServerErrorBody::invalid_params(
                        "This connection requires the bound Desktop client.",
                    ),
                )
                .await;
            }
            if params.capabilities.protocol_version != ProtocolVersion::CURRENT {
                return send_error(outgoing_tx, Some(id), AppServerErrorBody::new(
                    talos_app_protocol::errors::INVALID_PARAMS,
                    "Unsupported app-server protocol version; update the backend and client together.",
                )).await;
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
                return send_error(outgoing_tx, Some(id), AppServerErrorBody::not_initialized())
                    .await;
            }
            let actor = match resolve_request_actor(
                state.actor.as_ref(),
                validator.as_ref(),
                config.auth_enabled,
                decoded.method,
                request_actor_credentials,
            )
            .await
            {
                Ok(actor) => actor,
                Err(error) => return send_application_error(outgoing_tx, id, error).await,
            };
            state
                .subscriptions
                .subscribe(app.as_ref(), &actor, params, outgoing_tx, id)
                .await
        }
        ClientRequestParams::EventUnsubscribe(params) => {
            if !state.initialized_notification_seen {
                return send_error(outgoing_tx, Some(id), AppServerErrorBody::not_initialized())
                    .await;
            }
            if let Err(error) = resolve_request_actor(
                state.actor.as_ref(),
                validator.as_ref(),
                config.auth_enabled,
                decoded.method,
                request_actor_credentials,
            )
            .await
            {
                return send_application_error(outgoing_tx, id, error).await;
            }
            state
                .subscriptions
                .unsubscribe(params, outgoing_tx, id)
                .await
        }
        params => {
            let actor = match resolve_request_actor(
                state.actor.as_ref(),
                validator.as_ref(),
                config.auth_enabled,
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
    initialized_actor: Option<&Actor>,
    validator: &dyn ActorCredentialValidator,
    auth_enabled: bool,
    method: ClientMethod,
    request_actor_credentials: Option<ActorCredentials>,
) -> Result<Actor, ApiError> {
    if !auth_enabled {
        return Ok(Actor::auth_disabled());
    }

    if matches!(
        method,
        ClientMethod::DesktopWorkerIdentity
            | ClientMethod::DesktopWorkerEnroll
            | ClientMethod::AuthLoginStart
            | ClientMethod::AuthLoginStatus
            | ClientMethod::AuthLoginCancel
            | ClientMethod::AuthSessionRead
            | ClientMethod::AuthSessionClear
    ) && request_actor_credentials.is_none()
    {
        return initialized_actor
            .cloned()
            .ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated));
    }

    let credentials =
        request_actor_credentials.ok_or_else(|| ApiError::new(ErrorCode::Unauthenticated))?;
    validator.validate(&credentials).await
}

async fn forward_subscription(
    subscription_id: String,
    mut source: TimelineSubscription,
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    _subscription_capacity: usize,
) {
    let session_id = source.session_id.clone();
    while let Some(message) = source.events.recv().await {
        match message {
            TimelineSubscriptionMessage::ActiveTurnChanged(change) => {
                if send_active_turn_changed(change, &outgoing_tx)
                    .await
                    .is_err()
                {
                    return;
                }
            }
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
        Err(error) => send_error(outgoing_tx, Some(id), error).await,
    }
}

async fn send_application_error(
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    id: RequestId,
    error: ApiError,
) -> Result<(), ()> {
    let result = api_error_value(error).map_err(|error| {
        report_error(&TalosError::internal_with_source(
            "failed to serialize app-server application error",
            error,
        ));
    })?;
    outgoing_tx
        .send(OutgoingMessage::Response(JsonRpcResponse::new(id, result)))
        .await
        .map_err(|_| ())
}

fn api_success_value(result: serde_json::Value) -> Result<serde_json::Value, ()> {
    serde_json::to_value(ApiResponse::success(result)).map_err(|error| {
        report_error(&TalosError::internal_with_source(
            "failed to serialize app-server API success",
            error,
        ));
    })
}

fn api_error_value(error: ApiError) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::to_value(ApiResponse::<serde_json::Value>::error(error))
}

async fn send_error(
    outgoing_tx: mpsc::Sender<OutgoingMessage>,
    id: Option<RequestId>,
    error: AppServerErrorBody,
) -> Result<(), ()> {
    let code = match error.code {
        AppServerErrorCode::InternalError => ErrorCode::InternalError,
        AppServerErrorCode::ParseError
        | AppServerErrorCode::InvalidRequest
        | AppServerErrorCode::MethodNotFound
        | AppServerErrorCode::InvalidParams
        | AppServerErrorCode::NotInitialized
        | AppServerErrorCode::AlreadyInitialized => ErrorCode::InvalidRequest,
    };
    report_error(&TalosError::from_source(
        code,
        error.message.clone(),
        error.clone(),
    ));
    outgoing_tx
        .send(OutgoingMessage::Error(JsonRpcErrorResponse::new(id, error)))
        .await
        .map_err(|_| ())
}

async fn write_loop<W>(mut writer: W, mut rx: mpsc::Receiver<OutgoingMessage>) -> TalosResult<()>
where
    W: AsyncWrite + Unpin,
{
    while let Some(message) = rx.recv().await {
        let line = match message {
            OutgoingMessage::Request(request) => serde_json::to_string(&request),
            OutgoingMessage::Response(response) => serde_json::to_string(&response),
            OutgoingMessage::Error(error) => serde_json::to_string(&error),
            OutgoingMessage::Notification(notification) => serde_json::to_string(&notification),
        }
        .map_err(|error| {
            TalosError::internal_with_source("Failed to encode the app-server response.", error)
        })?;
        writer.write_all(line.as_bytes()).await.map_err(|error| {
            TalosError::internal_with_source("Failed to write the app-server response.", error)
        })?;
        writer.write_all(b"\n").await.map_err(|error| {
            TalosError::internal_with_source("Failed to write the app-server response.", error)
        })?;
        writer.flush().await.map_err(|error| {
            TalosError::internal_with_source("Failed to flush the app-server response.", error)
        })?;
    }
    Ok(())
}

async fn cancel_owned_turns(
    app: Arc<dyn AppServerApplication>,
    owned_turns: &HashMap<String, OwnedTurn>,
) {
    for (session_id, owned_turn) in owned_turns {
        // The application adapter already reports cancellation failures before
        // projecting ApiError; disconnected clients have no response receiver.
        let _ = app
            .cancel_active_turn(&owned_turn.actor, session_id, &owned_turn.turn_id)
            .await;
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
