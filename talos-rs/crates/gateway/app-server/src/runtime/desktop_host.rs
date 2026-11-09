//! Bounded Desktop host framing and correlation alongside concurrent request dispatch.
use std::sync::Arc;

use talos_app_protocol::jsonrpc::{
    parse_message_line, JsonRpcErrorResponse, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
    RequestId,
};
use talos_error::{ApiError, ErrorCode, TalosError};
use talos_logging::report_error;
use tokio::io::{AsyncRead, BufReader};
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

use super::{
    read_bounded_line, request_scheduler::RequestClass, send_application_error, BoundedLineRead,
    OutgoingMessage,
};

pub(super) const CONTROL_QUEUE_CAPACITY: usize = 4;

pub(super) type IncomingFrame = (
    std::io::Result<BoundedLineRead>,
    Vec<u8>,
    Option<OwnedSemaphorePermit>,
);

pub(super) async fn read_requests<R: AsyncRead + Unpin>(
    reader: R,
    max_incoming_frame_bytes: usize,
    host: Option<Arc<crate::DesktopHostBridge>>,
    reader_tx: mpsc::Sender<OutgoingMessage>,
    line_tx: mpsc::Sender<IncomingFrame>,
    stop: CancellationToken,
) {
    let queued_bytes = Arc::new(Semaphore::new(68 * 1024 * 1024));
    let control_bytes = Arc::new(Semaphore::new(1024 * 1024));
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        let result = tokio::select! {_=stop.cancelled()=>break,result=read_bounded_line(&mut reader,&mut line,max_incoming_frame_bytes)=>result};
        if matches!(result, Ok(BoundedLineRead::Eof)) {
            break;
        }
        if matches!(result, Ok(BoundedLineRead::Line)) {
            if let (Some(host), Ok(text)) = (&host, std::str::from_utf8(&line)) {
                if let Ok(message) = parse_message_line(text) {
                    match message {
                        JsonRpcMessage::Response(response) => {
                            complete_host_response(host, response);
                            continue;
                        }
                        JsonRpcMessage::Error(response) => {
                            complete_host_error(host, response);
                            continue;
                        }
                        _ => {}
                    }
                }
            }
        }
        let terminal = !matches!(result, Ok(BoundedLineRead::Line));
        let size = u32::try_from(line.len()).ok();
        let permit = if terminal {
            None
        } else if let Some(permit) = size.and_then(|size| {
            (line_tx.capacity() > CONTROL_QUEUE_CAPACITY)
                .then(|| queued_bytes.clone().try_acquire_many_owned(size).ok())
                .flatten()
        }) {
            Some(permit)
        } else if is_control_request(&line) {
            size.and_then(|size| control_bytes.clone().try_acquire_many_owned(size).ok())
        } else {
            None
        };
        if permit.is_none() && !terminal {
            reject_queued_request(&reader_tx, &line).await;
            continue;
        }
        let packet = (result, std::mem::take(&mut line), permit);
        if !terminal {
            match line_tx.try_send(packet) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full((_, frame, _))) => {
                    reject_queued_request(&reader_tx, &frame).await
                }
                Err(mpsc::error::TrySendError::Closed(_)) => break,
            }
        } else if tokio::select! {_=stop.cancelled()=>true,result=line_tx.send(packet)=>result.is_err()}
        {
            break;
        }
        if terminal {
            break;
        }
    }
    if let Some(host) = &host {
        host.close();
    }
}

pub(super) async fn forward_requests(
    mut browser_requests: Option<mpsc::Receiver<crate::DesktopHostEvent>>,
    host_tx: mpsc::Sender<OutgoingMessage>,
    stop: CancellationToken,
) {
    loop {
        let event = tokio::select! {_=stop.cancelled()=>break,event=async {match &mut browser_requests {Some(rx)=>rx.recv().await,None=>std::future::pending().await}}=>event};
        let Some(event) = event else {
            break;
        };
        let (id, method, params) = match event {
            crate::DesktopHostEvent::Media { id, frame } => {
                (id, "client/desktop/frame", serde_json::to_value(frame))
            }
            crate::DesktopHostEvent::Execute { id, request } => {
                (id, "client/browser/execute", serde_json::to_value(request))
            }
            crate::DesktopHostEvent::Cancel { id } => (
                uuid::Uuid::new_v4().to_string(),
                "client/browser/cancel",
                serde_json::to_value(talos_types::BrowserHostCancel { request_id: id }),
            ),
        };
        let params = match params {
            Ok(value) => value,
            Err(source) => {
                report_error(&TalosError::internal_with_source(
                    "Encode Desktop host request.",
                    source,
                ));
                stop.cancel();
                break;
            }
        };
        if tokio::select! {_=stop.cancelled()=>true,result=host_tx.send(OutgoingMessage::Request(JsonRpcRequest::new(RequestId::String(id),method,Some(params))))=>result.is_err()}
        {
            break;
        }
    }
    drop(host_tx);
}

fn is_control_request(line: &[u8]) -> bool {
    std::str::from_utf8(line)
        .ok()
        .and_then(|text| match parse_message_line(text).ok()? {
            JsonRpcMessage::Request(request) => request.method.parse().ok(),
            _ => None,
        })
        .is_some_and(|method| matches!(RequestClass::for_method(method), RequestClass::Control))
}

async fn reject_queued_request(outgoing: &mpsc::Sender<OutgoingMessage>, line: &[u8]) {
    if let Ok(text) = std::str::from_utf8(line) {
        if let Ok(JsonRpcMessage::Request(request)) = parse_message_line(text) {
            // A bounded Desktop queue rejects excess work without blocking host replies.
            let _ = send_application_error(
                outgoing.clone(),
                request.id,
                ApiError::new(ErrorCode::Conflict),
            )
            .await;
        }
    }
}

pub(super) fn complete_host_response(host: &crate::DesktopHostBridge, response: JsonRpcResponse) {
    if let RequestId::String(id) = response.id {
        host.complete(&id, Ok(response.result));
    }
}
pub(super) fn complete_host_error(host: &crate::DesktopHostBridge, response: JsonRpcErrorResponse) {
    if let Some(RequestId::String(id)) = response.id {
        host.complete(
            &id,
            Err(TalosError::new(
                ErrorCode::ToolExecutionFailed,
                "Desktop browser operation failed. Check its connection and current page.",
            )),
        );
    }
}
