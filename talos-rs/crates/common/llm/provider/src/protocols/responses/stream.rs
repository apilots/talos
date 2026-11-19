use super::{terminal, usage, ResponsesContract, ResponsesModelBlocks, ResponsesUsage};
use crate::{
    ChatChunk, ChatStream, ModelBlockDelta, ModelBlockStream, ModelBlockStreamEvent, SseLineDecoder,
};
use futures_util::StreamExt;
use serde_json::Value;
use talos_error::{TalosError, TalosResult};
use tokio::sync::mpsc;

mod blocks;
mod content;

use blocks::ResponseBlocks;

pub fn stream(response: reqwest::Response, provider_name: impl Into<String>) -> ChatStream {
    stream_with_contract(response, provider_name, ResponsesContract::Compatible)
}

/// Projects the shared Responses stream onto the plain-text API.
pub fn stream_with_contract(
    response: reqwest::Response,
    provider_name: impl Into<String>,
    contract: ResponsesContract,
) -> ChatStream {
    let provider_name = provider_name.into();
    let mut blocks = model_blocks_with_contract(
        response,
        ResponsesModelBlocks {
            item_prefix: provider_name.clone(),
            provider_name,
        },
        contract,
    );
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        while let Some(event) =
            tokio::select! { _ = tx.closed() => return, next = blocks.next() => next }
        {
            let chunk = match event {
                Ok(ModelBlockStreamEvent::BlockDelta {
                    delta: ModelBlockDelta::AssistantText { delta },
                    ..
                }) => Ok(ChatChunk {
                    delta,
                    finish_reason: None,
                }),
                Ok(ModelBlockStreamEvent::Done { finish_reason, .. }) => Ok(ChatChunk {
                    delta: String::new(),
                    finish_reason,
                }),
                Ok(_) => continue,
                Err(error) => Err(error),
            };
            if tx.send(chunk).await.is_err() {
                return;
            }
        }
    });
    ChatStream::new(rx)
}

pub fn model_blocks(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
) -> ModelBlockStream {
    model_blocks_with_contract(response, options, ResponsesContract::Compatible)
}

/// Decodes SSE into model blocks using the selected endpoint's termination contract.
pub fn model_blocks_with_contract(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
    contract: ResponsesContract,
) -> ModelBlockStream {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = decode_stream(response, options, contract, &tx).await;
        if let Err(error) = result {
            let _ = tx.send(Err(error)).await;
        }
    });
    ModelBlockStream::new(rx)
}

async fn decode_stream(
    response: reqwest::Response,
    options: ResponsesModelBlocks,
    contract: ResponsesContract,
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
) -> TalosResult<()> {
    let request_context = crate::error::response_context(&response);
    let mut stream = response.bytes_stream();
    let mut decoder = SseLineDecoder::new();
    let mut event_name = None;
    let mut blocks = ResponseBlocks::new(options.item_prefix, contract);
    let official = contract == ResponsesContract::Official;
    while let Some(chunk) =
        tokio::select! { _ = tx.closed() => return Ok(()), next = stream.next() => next }
    {
        let bytes = chunk.map_err(|error| {
            crate::error::transport_error("Model response stream", error).context(&request_context)
        })?;
        for line in decoder.push(&bytes) {
            let line = line?;
            if let Some(name) = line.strip_prefix("event:") {
                event_name = Some(name.trim().to_owned());
                continue;
            }
            if line.is_empty() {
                event_name = None;
            }
            let Some(data) = line
                .strip_prefix("data:")
                .map(str::trim_start)
                .filter(|data| !data.is_empty())
            else {
                continue;
            };
            if data == "[DONE]" {
                if official {
                    return Err(terminal::premature_done(&request_context));
                }
                let events = blocks.finish(&Value::Null, "stop".to_owned(), None)?;
                forward(tx, events).await;
                return Ok(());
            }
            let mut event: Value = serde_json::from_str(data).map_err(|error| {
                TalosError::internal_with_source(
                    "Model responses stream invalid SSE JSON; technical details were withheld.",
                    error,
                )
                .context(&options.provider_name)
                .context(&request_context)
            })?;
            if let Some((name, object)) = event_name.take().zip(event.as_object_mut()) {
                object.entry("type").or_insert(Value::String(name));
            }
            if let Some(terminal) =
                terminal::stream_terminal(&event, blocks.has_tool_calls(), contract)
            {
                let reason = terminal.map_err(|error| error.context(&request_context))?;
                let response = &event["response"];
                let usage = response
                    .get("usage")
                    .and_then(|value| serde_json::from_value::<ResponsesUsage>(value.clone()).ok())
                    .map(|value| usage(Some(value)));
                let events = blocks
                    .finish(response, reason, usage)
                    .map_err(|error| error.context(&request_context))?;
                forward(tx, events).await;
                return Ok(());
            }
            let events = blocks
                .apply(&event)
                .map_err(|error| error.context(&request_context))?;
            if !forward(tx, events).await {
                return Ok(());
            }
        }
    }
    if official {
        return Err(terminal::unterminated_stream(&request_context));
    }
    Ok(())
}

async fn forward(
    tx: &mpsc::Sender<TalosResult<ModelBlockStreamEvent>>,
    events: Vec<ModelBlockStreamEvent>,
) -> bool {
    for event in events {
        if tx.send(Ok(event)).await.is_err() {
            return false;
        }
    }
    true
}
