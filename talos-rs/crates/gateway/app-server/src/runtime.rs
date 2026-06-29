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
#[path = "runtime_tests.rs"]
mod tests;
