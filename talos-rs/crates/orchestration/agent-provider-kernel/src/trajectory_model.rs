//! Normalized model snapshots and streaming response observation.
use super::{bounded_json_size, tool_call, Capture};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use talos_agent_api::trajectory::*;
use talos_agent_kernel::trajectory::KernelModelObservation;
use talos_error::{ErrorCode, TalosError};
use talos_llm_provider::{
    ChatMessage, ChatResponse, FunctionCall, MessageInputKind, MessageInputSource, ModelBlockDelta,
    ModelBlockKind, ModelBlockStreamEvent, ReasoningBlock, ReasoningContinuation, Role,
};
use talos_types::TokenUsage;

#[derive(Debug)]
pub(super) struct ModelCapture {
    capture: Arc<Capture>,
    state: Mutex<ModelState>,
}
#[derive(Debug)]
struct ModelState {
    detail: TrajectoryReadModelRequestResponse,
    parts: Vec<ResponsePart>,
    blocks: BTreeMap<String, usize>,
    tool_indexes: BTreeMap<usize, usize>,
    bytes: usize,
    reserved_bytes: usize,
    done: bool,
    finished: bool,
    complete_body: bool,
}
impl ModelCapture {
    pub(super) fn new(capture: Arc<Capture>, detail: TrajectoryReadModelRequestResponse) -> Self {
        Self {
            capture,
            state: Mutex::new(ModelState {
                detail,
                parts: vec![],
                blocks: BTreeMap::new(),
                tool_indexes: BTreeMap::new(),
                bytes: 0,
                reserved_bytes: 0,
                done: false,
                finished: false,
                complete_body: true,
            }),
        }
    }

    fn finish_inner(&self, error: Option<&TalosError>, interrupted: bool) {
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        if s.finished {
            return;
        }
        s.finished = true;
        let now = self.capture.now();
        let done = s.done;
        let completed = done && error.is_none() && !interrupted;
        let attempt = &mut s.detail.request.call_records[0];
        if !done {
            if let Some(timing) = &mut attempt.timing {
                if !interrupted {
                    timing.interval.ended_offset_ms = Some(now);
                    timing.interval.elapsed_ms = Some(now - timing.interval.started_offset_ms);
                }
            }
        }
        attempt.observed_state = if error.is_some_and(|e| e.is_code(ErrorCode::Cancelled)) {
            ModelCallState::Cancelled
        } else if error.is_some() {
            ModelCallState::Failed
        } else if completed {
            ModelCallState::Completed
        } else {
            ModelCallState::Incomplete
        };
        attempt.error = error.map(TalosError::to_api_error);
        let complete = done && s.complete_body;
        let response = if s.complete_body || !s.parts.is_empty() {
            self.capture.payload_with_reserved_bytes(
                TrajectoryPayload::Response {
                    parts: std::mem::take(&mut s.parts),
                },
                Some(complete),
                s.reserved_bytes,
            )
        } else {
            None
        };
        s.detail.outputs = vec![ModelCallOutput {
            call_record_id: s.detail.request.call_records[0].call_record_id.clone(),
            response,
        }];
        self.capture.request_finished(s.detail.clone());
    }
}
impl KernelModelObservation for ModelCapture {
    fn tool_calls(&self, calls: &[FunctionCall]) {
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        let call_bytes = if s.tool_indexes.is_empty() {
            Some(0)
        } else if s.complete_body {
            bounded_json_size(&calls, 4 * 1024 * 1024).filter(|bytes| self.capture.reserve(*bytes))
        } else {
            None
        };
        let can_freeze_calls = call_bytes.is_some();
        s.reserved_bytes += call_bytes.unwrap_or(0);
        if !can_freeze_calls {
            s.complete_body = false;
        }
        let ModelState {
            parts,
            tool_indexes,
            ..
        } = &mut *s;
        for (call, position) in calls.iter().zip(tool_indexes.values().copied()) {
            let raw = match &parts[position] {
                ResponsePart::ToolCall { call } => call.arguments_text.clone(),
                _ => None,
            };
            if can_freeze_calls {
                parts[position] = ResponsePart::ToolCall {
                    call: tool_call(call, raw),
                };
            } else if let ResponsePart::ToolCall { call: saved } = &mut parts[position] {
                saved.call_id = call.id.clone();
                saved.name = call.name.clone();
            }
        }
        for call in calls {
            self.capture.link_tool(
                &call.id,
                &s.detail.request.model_request_id,
                &s.detail.request.call_records[0].call_record_id,
            );
        }
    }
    fn model_event(&self, event: &ModelBlockStreamEvent) {
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        let now = self.capture.now();
        match event {
            ModelBlockStreamEvent::BlockStarted(b) => {
                if s.parts.len() >= 1024 {
                    s.complete_body = false;
                    return;
                }
                let part = match b.kind {
                    ModelBlockKind::Reasoning => Some(ResponsePart::Reasoning {
                        text: String::new(),
                    }),
                    ModelBlockKind::AssistantText => Some(ResponsePart::Text {
                        text: String::new(),
                    }),
                    _ => None,
                };
                if let Some(part) = part {
                    let index = s.parts.len();
                    s.blocks.insert(b.model_item_id.clone(), index);
                    s.parts.push(part);
                }
                if let Some(call) = &b.tool_call {
                    let index = s.parts.len();
                    s.blocks.insert(b.model_item_id.clone(), index);
                    s.tool_indexes.insert(call.index, index);
                    s.parts.push(ResponsePart::ToolCall {
                        call: SnapshotToolCall {
                            call_id: call.call_id.clone().unwrap_or_default(),
                            name: call.tool_name.clone().unwrap_or_default(),
                            arguments: serde_json::Value::Null.into(),
                            arguments_text: None,
                        },
                    });
                }
            }
            ModelBlockStreamEvent::BlockDelta {
                model_item_id,
                delta,
            } => {
                let text = match delta {
                    ModelBlockDelta::ReasoningText { delta }
                    | ModelBlockDelta::AssistantText { delta }
                    | ModelBlockDelta::ToolArguments { delta } => Some(delta),
                    ModelBlockDelta::ToolProgress { .. } => None,
                };
                if let Some(text) = text {
                    if !text.is_empty() {
                        if let Some(timing) = &mut s.detail.request.call_records[0].timing {
                            timing.first_output_offset_ms.get_or_insert(now);
                            timing.last_output_offset_ms = Some(now);
                            timing.ttft_ms = timing
                                .first_output_offset_ms
                                .map(|first| first - timing.interval.started_offset_ms);
                            timing.generation_ms =
                                timing.first_output_offset_ms.map(|first| now - first);
                        }
                    }
                    if s.complete_body
                        && s.bytes.saturating_add(text.len()) <= 4 * 1024 * 1024
                        && self.capture.reserve(text.len())
                    {
                        s.bytes += text.len();
                        s.reserved_bytes += text.len();
                        if let Some(index) = s.blocks.get(model_item_id).copied() {
                            match &mut s.parts[index] {
                                ResponsePart::Text { text: saved }
                                | ResponsePart::Reasoning { text: saved } => saved.push_str(text),
                                ResponsePart::ToolCall { call } => {
                                    call.arguments_text.get_or_insert_default().push_str(text)
                                }
                            }
                        }
                    } else {
                        s.complete_body = false;
                    }
                }
            }
            ModelBlockStreamEvent::BlockCompleted(b) => {
                if let Some(index) = s.blocks.get(&b.model_item_id).copied() {
                    if let ResponsePart::ToolCall { call } = &mut s.parts[index] {
                        if let Some(raw) = &call.arguments_text {
                            call.arguments = serde_json::from_str::<serde_json::Value>(raw)
                                .unwrap_or_else(|_| serde_json::Value::String(raw.clone()))
                                .into();
                        }
                    }
                }
            }
            ModelBlockStreamEvent::Done {
                finish_reason,
                usage,
            } => {
                s.done = true;
                let a = &mut s.detail.request.call_records[0];
                a.finish_reason = finish_reason.clone();
                a.usage = usage.as_ref().map(TokenUsage::from);
                if let Some(t) = &mut a.timing {
                    t.interval.ended_offset_ms = Some(now);
                    t.interval.elapsed_ms = Some(now - t.interval.started_offset_ms);
                }
            }
            ModelBlockStreamEvent::ReasoningCompleted { .. } => {}
        }
    }
    fn response(&self, response: &ChatResponse) {
        let Ok(mut s) = self.state.try_lock() else {
            return;
        };
        let reasoning = response.reasoning.display_text();
        let raw_bytes = response.get_tool_calls().iter().fold(0usize, |n, call| {
            n.saturating_add(call.arguments_text.as_ref().map_or(0, String::len))
        });
        let body_bytes = response
            .content
            .len()
            .saturating_add(reasoning.len())
            .saturating_add(raw_bytes);
        let reserved_bytes = (body_bytes <= 4 * 1024 * 1024)
            .then(|| bounded_json_size(&response.get_tool_calls(), 4 * 1024 * 1024))
            .flatten()
            .map(|call_bytes| body_bytes.saturating_add(call_bytes))
            .filter(|bytes| self.capture.reserve(*bytes));
        let fits = reserved_bytes.is_some();
        s.reserved_bytes += reserved_bytes.unwrap_or(0);
        s.complete_body = fits;
        if fits {
            if !reasoning.is_empty() {
                s.parts.push(ResponsePart::Reasoning { text: reasoning });
            }
            if !response.content.is_empty() {
                s.parts.push(ResponsePart::Text {
                    text: response.content.clone(),
                });
            }
            s.parts.extend(
                response
                    .get_tool_calls()
                    .iter()
                    .map(|c| ResponsePart::ToolCall {
                        call: tool_call(c, None),
                    }),
            );
        }
        s.done = true;
        let now = self.capture.now();
        let a = &mut s.detail.request.call_records[0];
        a.usage = Some(TokenUsage::from(&response.usage));
        a.finish_reason = response.finish_reason.clone();
        if let Some(t) = &mut a.timing {
            t.interval.ended_offset_ms = Some(now);
            t.interval.elapsed_ms = Some(now - t.interval.started_offset_ms);
        }
    }
    fn finished(&self, error: Option<&TalosError>) {
        self.finish_inner(error, false);
    }
    fn interrupted(&self) {
        self.finish_inner(None, true);
    }
}
pub(super) fn snapshot_message(m: &ChatMessage, index: u32) -> SnapshotMessage {
    SnapshotMessage {
        index,
        role: match m.role {
            Role::System => SnapshotRole::System,
            Role::User => SnapshotRole::User,
            Role::Assistant => SnapshotRole::Assistant,
            Role::Tool => SnapshotRole::Tool,
        },
        content: m.content.clone(),
        reasoning: m
            .reasoning_continuation
            .as_ref()
            .map(|continuation| match continuation {
                ReasoningContinuation::Text(text) => {
                    vec![SnapshotReasoningPart::Text { text: text.clone() }]
                }
                ReasoningContinuation::Blocks(blocks) => blocks
                    .iter()
                    .filter_map(|block| match block {
                        ReasoningBlock::Text(block) => Some(SnapshotReasoningPart::Text {
                            text: block.text.clone(),
                        }),
                        ReasoningBlock::Summary(block) => Some(SnapshotReasoningPart::Summary {
                            text: block.text.clone(),
                        }),
                        ReasoningBlock::Encrypted(_) => None,
                    })
                    .collect(),
            }),
        name: m.name.clone(),
        tool_call_id: m.tool_call_id.clone(),
        tool_calls: m
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|c| tool_call(c, None)).collect()),
        attachments: Some(
            m.attachments
                .as_ref()
                .map(|a| {
                    a.iter()
                        .enumerate()
                        .map(|(index, a)| SnapshotAttachment {
                            file_id: a
                                .file_id
                                .clone()
                                .or_else(|| m.image_file_ids.get(index).cloned()),
                            filename: Some(a.filename.clone()),
                            kind: Some(match a.kind {
                                talos_llm_provider::AttachmentKind::Image => {
                                    SnapshotAttachmentKind::Image
                                }
                                talos_llm_provider::AttachmentKind::File => {
                                    SnapshotAttachmentKind::File
                                }
                            }),
                            mime_type: Some(a.media_type.clone()),
                            size_bytes: Some(a.bytes.len() as f64),
                            availability: SnapshotAvailability::NotRetained,
                        })
                        .collect()
                })
                .unwrap_or_else(|| {
                    m.image_file_ids
                        .iter()
                        .map(|id| SnapshotAttachment {
                            file_id: Some(id.clone()),
                            filename: None,
                            kind: Some(SnapshotAttachmentKind::Image),
                            mime_type: None,
                            size_bytes: None,
                            availability: SnapshotAvailability::NotRetained,
                        })
                        .collect()
                }),
        ),
        cache_control: Some(if m.cache_control.is_some() {
            SnapshotCacheControl::Ephemeral
        } else {
            SnapshotCacheControl::None
        }),
        input_origin: m.input_origin.map(|origin| SnapshotInputOrigin {
            kind: match origin.kind {
                MessageInputKind::System => SnapshotInputKind::System,
                MessageInputKind::User => SnapshotInputKind::User,
                MessageInputKind::Context => SnapshotInputKind::Context,
            },
            source: match origin.source {
                MessageInputSource::User => SnapshotInputSource::User,
                MessageInputSource::Prompt => SnapshotInputSource::Prompt,
                MessageInputSource::Runtime => SnapshotInputSource::Runtime,
                MessageInputSource::Workspace => SnapshotInputSource::Workspace,
                MessageInputSource::Skills => SnapshotInputSource::Skills,
                MessageInputSource::Memory => SnapshotInputSource::Memory,
                MessageInputSource::Mixed => SnapshotInputSource::Mixed,
            },
            admission: SnapshotInputAdmission::Unknown,
        }),
    }
}
