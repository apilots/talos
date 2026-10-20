//! One invocation's CDP transport. Every actual outgoing command uses the Worker input fence.
use crate::BrowserDispatch;
use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::net::TcpListener;
use tokio_tungstenite::{
    accept_hdr_async_with_config, connect_async_with_config,
    tungstenite::{
        handshake::server::{ErrorResponse, Request, Response},
        protocol::{Message, WebSocketConfig},
    },
};
use tokio_util::sync::CancellationToken;

pub(crate) struct InvocationProxy {
    pub url: String,
    stop: CancellationToken,
    _task: tokio::task::JoinHandle<()>,
}
impl InvocationProxy {
    pub async fn start(
        endpoint: String,
        dispatch: BrowserDispatch,
        read_only: bool,
        pressed: crate::pressed::PressedInputs,
    ) -> TalosResult<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Failed to prepare the browser invocation transport.",
                    error,
                )
            })?;
        let address = listener.local_addr().map_err(|error| {
            TalosError::internal_with_source(
                "Failed to inspect the browser invocation transport.",
                error,
            )
        })?;
        let path = Arc::new(format!("/{}", uuid::Uuid::new_v4().simple()));
        let url = format!("ws://{address}{path}");
        let stop = CancellationToken::new();
        let ended = stop.clone();
        let task = tokio::spawn(async move {
            let operation = async move {
                let (socket, peer) = listener.accept().await.map_err(|error| {
                    TalosError::internal_with_source(
                        "Browser invocation transport is unavailable.",
                        error,
                    )
                })?;
                if !peer.ip().is_loopback() {
                    return Err(denied());
                }
                let authorize = move |request: &Request,
                                      response: Response|
                      -> Result<Response, ErrorResponse> {
                    if request.uri().path() != path.as_str()
                        || request.uri().query().is_some()
                        || request.headers().contains_key("origin")
                    {
                        let mut error =
                            ErrorResponse::new(Some("Browser transport rejected.".into()));
                        *error.status_mut() =
                            tokio_tungstenite::tungstenite::http::StatusCode::FORBIDDEN;
                        Err(error)
                    } else {
                        Ok(response)
                    }
                };
                let mut client = accept_hdr_async_with_config(socket, authorize, Some(config()))
                    .await
                    .map_err(|error| {
                        TalosError::from_source(
                            ErrorCode::PermissionDenied,
                            "Browser transport handshake was rejected.",
                            error,
                        )
                    })?;
                dispatch.check()?;
                let (mut browser, _) = connect_async_with_config(endpoint, Some(config()), false)
                    .await
                    .map_err(|error| {
                        TalosError::from_source(
                            ErrorCode::ServiceUnavailable,
                            "The managed browser connection is unavailable.",
                            error,
                        )
                    })?;
                let cancellation = dispatch.cancellation()?;
                let mut sessions = std::collections::HashMap::new();
                let mut attachments = std::collections::HashMap::new();
                let mut releases = std::collections::HashMap::new();
                loop {
                    tokio::select! {
                        _=cancellation.cancelled()=>return Ok(()),
                        incoming=client.next()=>{
                            let Some(incoming)=incoming else {return Ok(());};
                            let message=incoming.map_err(|error|TalosError::internal_with_source("Browser driver transport failed.",error))?;
                            match &message {
                                Message::Text(text)=>{
                                    let command:serde_json::Value=serde_json::from_str(text).map_err(|error|TalosError::from_source(ErrorCode::InvalidRequest,"Browser driver command is invalid.",error))?;
                                    let method=command.get("method").and_then(serde_json::Value::as_str).ok_or_else(denied)?;
                                    if read_only && input_method(method) {return Err(denied());}
                                    // Held through the bounded send. Pause waits for an in-flight send, then clears pressed keys.
                                    let _permit=dispatch.input_permit().await?;
                                    if method == "Target.attachToTarget" {
                                        if attachments.len()>=256 {return Err(denied());}
                                        if let (Some(id),Some(target))=(command.get("id").and_then(serde_json::Value::as_u64),command.pointer("/params/targetId").and_then(serde_json::Value::as_str)) {attachments.insert(id,target.to_owned());}
                                    }
                                    if let Some(release)=pressed.before_send(&command,&sessions)? {
                                        let id=command.get("id").and_then(serde_json::Value::as_u64).ok_or_else(denied)?;
                                        let session=command.get("sessionId").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                                        if releases.len()>=256 || releases.insert((id,session),release).is_some(){return Err(denied());}
                                    }
                                    tokio::time::timeout(Duration::from_secs(2),browser.send(message)).await
                                        .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Browser input transport timed out.",error))?
                                        .map_err(|error|TalosError::internal_with_source("Browser command dispatch failed.",error))?;
                                },
                                Message::Close(_)=>return Ok(()), Message::Ping(_) | Message::Pong(_)=>{}, _=>return Err(denied()),
                            }
                        },
                        incoming=browser.next()=>{
                            let Some(incoming)=incoming else {return Ok(());};
                            let message=incoming.map_err(|error|TalosError::internal_with_source("The managed browser transport failed.",error))?;
                            if message.is_close() {return Ok(());}
                            if let Message::Text(text)=&message {
                                if let Ok(reply)=serde_json::from_str::<serde_json::Value>(text) {
                                    if let Some(id)=reply.get("id").and_then(serde_json::Value::as_u64) {
                                        let session=reply.get("sessionId").and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
                                        if let Some(release)=releases.remove(&(id,session)) {
                                            if reply.get("result").is_some() && reply.get("error").is_none(){pressed.confirm_release(release)?;}
                                        }
                                    }
                                    if let Some(target)=reply.get("id").and_then(serde_json::Value::as_u64).and_then(|id|attachments.remove(&id)) {
                                        if let Some(session)=reply.pointer("/result/sessionId").and_then(serde_json::Value::as_str) {
                                            if sessions.len()>=256{return Err(denied());}sessions.insert(session.to_owned(),target);
                                        }
                                    }
                                }
                            }
                            tokio::time::timeout(Duration::from_secs(2),client.send(message)).await
                                .map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Browser driver is not receiving results.",error))?
                                .map_err(|error|TalosError::internal_with_source("Browser result dispatch failed.",error))?;
                        },
                    }
                }
            };
            let result = tokio::select! {
                _=ended.cancelled()=>Ok(()),
                result=tokio::time::timeout(Duration::from_secs(120),operation)=>
                    result.map_err(|error|TalosError::from_source(ErrorCode::Timeout,"Browser invocation transport expired.",error)).and_then(|value|value),
            };
            if let Err(error) = result {
                talos_logging::report_error(&error);
            }
        });
        Ok(Self {
            url,
            stop,
            _task: task,
        })
    }
}
impl Drop for InvocationProxy {
    fn drop(&mut self) {
        self.stop.cancel();
        // Cancellation wakes the task so its handling owner can report errors.
        // Dropping the handle detaches it; no in-flight result is discarded.
    }
}
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(32 * 1024)
        .write_buffer_size(32 * 1024)
        .max_write_buffer_size(512 * 1024)
        .max_message_size(Some(8 * 1024 * 1024))
        .max_frame_size(Some(8 * 1024 * 1024))
}
fn input_method(method: &str) -> bool {
    method.starts_with("Input.")
        || matches!(
            method,
            "Page.navigate"
                | "Page.reload"
                | "Page.handleJavaScriptDialog"
                | "Page.navigateToHistoryEntry"
                | "Target.activateTarget"
                | "Target.createTarget"
                | "Target.closeTarget"
                | "Browser.close"
                | "Browser.setDownloadBehavior"
                | "DOM.setFileInputFiles"
                | "DOM.scrollIntoViewIfNeeded"
                | "Runtime.evaluate"
                | "Runtime.callFunctionOn"
        )
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "This browser invocation cannot send that command.",
    )
}

/// Internal cleanup has a closed command set; it cannot evaluate or navigate.
pub(crate) async fn release(
    endpoint: String,
    held: std::collections::BTreeMap<String, Vec<serde_json::Value>>,
) -> TalosResult<()> {
    let operation = async {
        let (mut browser, _) = connect_async_with_config(endpoint, Some(config()), false)
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Browser input cleanup connection failed.", error)
            })?;
        let mut id = 0u64;
        for (target, events) in held {
            id += 1;
            let attach_id = id;
            browser.send(Message::Text(serde_json::json!({"id":id,"method":"Target.attachToTarget","params":{"targetId":target,"flatten":true}}).to_string().into())).await
                .map_err(|error|TalosError::internal_with_source("Browser input cleanup could not attach its target.",error))?;
            let session = loop {
                let response = browser
                    .next()
                    .await
                    .ok_or_else(|| {
                        TalosError::internal("Browser input cleanup connection closed.")
                    })?
                    .map_err(|error| {
                        TalosError::internal_with_source("Browser input cleanup failed.", error)
                    })?;
                if let Message::Text(text) = response {
                    let reply: serde_json::Value =
                        serde_json::from_str(&text).map_err(|error| {
                            TalosError::internal_with_source(
                                "Browser cleanup response is invalid.",
                                error,
                            )
                        })?;
                    if reply.get("id").and_then(serde_json::Value::as_u64) == Some(attach_id) {
                        break reply
                            .pointer("/result/sessionId")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .ok_or_else(|| {
                                TalosError::internal("Browser cleanup target is unavailable.")
                            })?;
                    }
                }
            };
            for event in events {
                id += 1;
                let method =
                    if event.get("type").and_then(serde_json::Value::as_str) == Some("keyUp") {
                        "Input.dispatchKeyEvent"
                    } else {
                        "Input.dispatchMouseEvent"
                    };
                browser.send(Message::Text(serde_json::json!({"id":id,"sessionId":session,"method":method,"params":event}).to_string().into())).await
                    .map_err(|error|TalosError::internal_with_source("Browser pressed input could not be released.",error))?;
                loop {
                    let response = browser
                        .next()
                        .await
                        .ok_or_else(|| {
                            TalosError::internal("Browser input cleanup connection closed.")
                        })?
                        .map_err(|error| {
                            TalosError::internal_with_source(
                                "Browser input release acknowledgement failed.",
                                error,
                            )
                        })?;
                    if let Message::Text(text) = response {
                        let reply: serde_json::Value =
                            serde_json::from_str(&text).map_err(|error| {
                                TalosError::internal_with_source(
                                    "Browser cleanup acknowledgement is invalid.",
                                    error,
                                )
                            })?;
                        if reply.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
                            if reply.get("error").is_some() {
                                return Err(TalosError::internal(
                                    "Browser input release was rejected.",
                                ));
                            }
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    };
    tokio::time::timeout(Duration::from_secs(4), operation)
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser pressed input release timed out.",
                error,
            )
        })?
}

pub(crate) async fn close_browser(endpoint: String) -> TalosResult<()> {
    let operation = async {
        let (mut browser, _) = connect_async_with_config(endpoint, Some(config()), false)
            .await
            .map_err(|error| {
                TalosError::internal_with_source("Browser profile flush connection failed.", error)
            })?;
        browser
            .send(Message::Text(
                serde_json::json!({"id":1,"method":"Browser.close"})
                    .to_string()
                    .into(),
            ))
            .await
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Browser profile flush could not be requested.",
                    error,
                )
            })?;
        loop {
            match browser.next().await {
                None | Some(Ok(Message::Close(_))) => return Ok(()),
                Some(Ok(Message::Text(text))) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).map_err(|error| {
                            TalosError::internal_with_source(
                                "Browser close acknowledgement is invalid.",
                                error,
                            )
                        })?;
                    if value.get("id").and_then(serde_json::Value::as_u64) == Some(1) {
                        return if value.get("error").is_some() {
                            Err(TalosError::internal("Browser profile flush was rejected."))
                        } else {
                            Ok(())
                        };
                    }
                }
                Some(Err(error)) => {
                    return Err(TalosError::internal_with_source(
                        "Browser profile flush acknowledgement failed.",
                        error,
                    ))
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(3), operation)
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::Timeout,
                "Browser profile flush timed out.",
                error,
            )
        })?
}
