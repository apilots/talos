//! Invocation-local missing facts, handed to the existing Session history owner.
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use talos_agent::SessionHistoryReader;
use talos_agent_api::{trajectory::*, AgentTurnRef};
use talos_agent_kernel::trajectory::{
    KernelModelObservation, KernelObservedPurpose, KernelTrajectoryRecorder,
};
use talos_llm_provider::{ChatMessage, ChatRequest, FunctionCall, Role};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
#[path = "trajectory_model.rs"]
mod model;
use model::{snapshot_message, ModelCapture};

pub(crate) struct Capture {
    identity: AgentTurnRef,
    clock: ClockSegment,
    started: Instant,
    started_at_ms: i64,
    next_request: AtomicU32,
    tools: Mutex<BTreeMap<String, TrajectoryReadToolResponse>>,
    supplement: Mutex<SessionHistorySupplement>,
    reservations: Mutex<Option<OwnedSemaphorePermit>>,
}
impl std::fmt::Debug for Capture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Capture")
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}
impl Capture {
    pub(crate) fn new(identity: AgentTurnRef, resumed: bool, node_id: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            supplement: Mutex::new(SessionHistorySupplement {
                identity: identity.clone(),
                clock_segments: vec![],
                requests: vec![],
                tools: vec![],
                payloads: BTreeMap::new(),
            }),
            clock: ClockSegment {
                segment_id: Uuid::new_v4().to_string(),
                source_node_id: node_id.unwrap_or_default(),
                started_at: chrono::Utc::now().timestamp(),
                relation: if resumed {
                    ClockRelation::Resumed
                } else {
                    ClockRelation::TurnStart
                },
                observed_through_offset_ms: 0.0,
            },
            identity,
            started: Instant::now(),
            started_at_ms: chrono::Utc::now().timestamp_millis(),
            next_request: AtomicU32::new(1),
            tools: Mutex::new(BTreeMap::new()),
            reservations: Mutex::new(None),
        })
    }
    fn now(&self) -> f64 {
        self.started_at_ms as f64 + self.started.elapsed().as_secs_f64() * 1000.0
    }
    fn reserve(&self, bytes: usize) -> bool {
        static PROCESS_BUDGET: OnceLock<Arc<Semaphore>> = OnceLock::new();
        let process = PROCESS_BUDGET.get_or_init(|| Arc::new(Semaphore::new(32 * 1024 * 1024)));
        let size = bytes.saturating_mul(2).max(1) as u32;
        let Ok(process) = process.clone().try_acquire_many_owned(size) else {
            return false;
        };
        let Ok(mut reservations) = self.reservations.try_lock() else {
            return false;
        };
        if let Some(existing) = reservations.as_mut() {
            existing.merge(process);
        } else {
            *reservations = Some(process);
        }
        true
    }
    fn payload(&self, value: TrajectoryPayload, complete: Option<bool>) -> Option<PayloadRef> {
        self.payload_with_reserved_bytes(value, complete, 0)
    }
    // Moving a model body into its final snapshot retains the capacity already
    // admitted while collecting it; only serialization overhead needs more.
    fn payload_with_reserved_bytes(
        &self,
        value: TrajectoryPayload,
        complete: Option<bool>,
        reserved_bytes: usize,
    ) -> Option<PayloadRef> {
        let size = bounded_json_size(&value, 4 * 1024 * 1024)?;
        let mut supplement = self.supplement.try_lock().ok()?;
        if matches!(
            value,
            TrajectoryPayload::Tools { .. } | TrajectoryPayload::Parameters { .. }
        ) {
            if let Some((id, _)) = supplement.payloads.iter().find(|(_, old)| *old == &value) {
                return Some(PayloadRef {
                    payload_id: id.clone(),
                    complete,
                });
            }
        }
        if !self.reserve(size.saturating_sub(reserved_bytes)) {
            return None;
        }
        let id = Uuid::new_v4().to_string();
        supplement.payloads.insert(id.clone(), value);
        Some(PayloadRef {
            payload_id: id,
            complete,
        })
    }
    fn prepared_equals(&self, id: &str, text: &str) -> Option<bool> {
        let supplement = self.supplement.try_lock().ok()?;
        match supplement.payloads.get(id)? {
            TrajectoryPayload::PreparedResult { message }
                if message.attachments.as_ref().is_none_or(Vec::is_empty) =>
            {
                Some(message.content == text)
            }
            _ => None,
        }
    }
    fn request_finished(&self, detail: TrajectoryReadModelRequestResponse) {
        if bounded_json_size(&detail, 256 * 1024).is_none() {
            return;
        }
        if let Ok(mut supplement) = self.supplement.try_lock() {
            if supplement.requests.len() < 128 {
                supplement.requests.push(detail);
            }
        }
    }
    pub(crate) async fn flush(&self, history: &dyn SessionHistoryReader) {
        let Some(supplement) = self.take() else {
            return;
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            history.append_supplement(supplement),
        )
        .await;
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(error)) => talos_logging::report_error(&error),
            Err(_) => talos_logging::report_error(&talos_error::TalosError::new(
                talos_error::ErrorCode::Timeout,
                "Optional execution history handoff exceeded its deadline.",
            )),
        }
    }
    fn take(&self) -> Option<SessionHistorySupplement> {
        let mut stored = self.supplement.lock().ok()?;
        let mut supplement = SessionHistorySupplement {
            identity: self.identity.clone(),
            clock_segments: vec![],
            requests: std::mem::take(&mut stored.requests),
            tools: vec![],
            payloads: std::mem::take(&mut stored.payloads),
        };
        supplement.tools = std::mem::take(&mut *self.tools.lock().ok()?)
            .into_values()
            .collect();
        let requests: std::collections::HashSet<_> = supplement
            .requests
            .iter()
            .map(|r| r.request.model_request_id.as_str())
            .collect();
        for tool in &mut supplement.tools {
            if tool
                .tool
                .source_model_request_id
                .as_deref()
                .is_some_and(|id| !requests.contains(id))
            {
                tool.tool.source_model_request_id = None;
                tool.tool.source_call_record_id = None;
            }
            if let Some(usages) = &mut tool.usages {
                usages.retain(|usage| requests.contains(usage.model_request_id.as_str()));
            }
        }
        if supplement.requests.is_empty() && supplement.tools.is_empty() {
            return None;
        }
        let mut clock = self.clock.clone();
        clock.observed_through_offset_ms = self.now() - self.clock.started_at as f64 * 1000.0;
        supplement.clock_segments.push(clock);
        Some(supplement)
    }
    pub(crate) fn recorder(self: &Arc<Self>) -> Arc<dyn KernelTrajectoryRecorder> {
        Arc::new(InvocationRecorder(self.clone()))
    }
}
#[derive(Debug)]
struct InvocationRecorder(Arc<Capture>);
fn tool_call(call: &FunctionCall, raw: Option<String>) -> SnapshotToolCall {
    SnapshotToolCall {
        call_id: call.id.clone(),
        name: call.name.clone(),
        arguments: call.arguments.clone().into(),
        arguments_text: raw.or_else(|| call.arguments_text.clone()),
    }
}
impl KernelTrajectoryRecorder for InvocationRecorder {
    fn prepared_tool_result(&self, message: &ChatMessage) {
        if let Some(call_id) = &message.tool_call_id {
            self.0.prepare_tool(call_id, message);
        }
    }

    fn start_model(
        &self,
        mode: KernelObservedPurpose,
        connection_id: &str,
        request: &ChatRequest,
    ) -> Option<Arc<dyn KernelModelObservation>> {
        let capture = &self.0;
        let ordinal = capture.next_request.fetch_add(1, Ordering::AcqRel);
        if ordinal > 128 {
            return None;
        }
        // Each input region has its own admission. Missing one large body must
        // not hide other independently known model options or terminal metrics.
        let prompt_fits = bounded_json_size(&request.messages, 4 * 1024 * 1024).is_some();
        let tools_fit = bounded_json_size(&request.tools, 4 * 1024 * 1024).is_some();
        let options_fit = bounded_json_size(
            &(&request.request_parameters, &request.stop),
            4 * 1024 * 1024,
        )
        .is_some();
        let prompt = prompt_fits
            .then(|| {
                let messages = request
                    .messages
                    .iter()
                    .enumerate()
                    .map(|(index, m)| snapshot_message(m, index as u32))
                    .collect();
                capture.payload(TrajectoryPayload::Prompt { messages }, Some(true))
            })
            .flatten();
        let tools = tools_fit
            .then(|| {
                capture.payload(
                    TrajectoryPayload::Tools {
                        definitions: request
                            .tools
                            .as_ref()
                            .map(|defs| {
                                defs.iter()
                                    .map(|d| SnapshotToolDefinition {
                                        name: d.name.clone(),
                                        description: d.description.clone(),
                                        parameters: d.parameters.clone().map(Into::into),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    },
                    Some(true),
                )
            })
            .flatten();
        let parameters = options_fit
            .then(|| {
                capture.payload(
                    TrajectoryPayload::Parameters {
                        options: NormalizedRequestOptions {
                            temperature: request.temperature.map(f64::from),
                            max_tokens: request.max_tokens,
                            top_p: request.top_p.map(f64::from),
                            stop: request.stop.clone(),
                            parallel_tool_calls: request.parallel_tool_calls,
                            tool_choice: request.tool_choice.as_ref().map(|t| match t {
                                talos_llm_provider::ToolChoice::Auto => SnapshotToolChoice::Auto,
                                talos_llm_provider::ToolChoice::None => SnapshotToolChoice::None,
                                talos_llm_provider::ToolChoice::Required => {
                                    SnapshotToolChoice::Required
                                }
                                talos_llm_provider::ToolChoice::Named { name } => {
                                    SnapshotToolChoice::Named { name: name.clone() }
                                }
                            }),
                            reasoning_effort: request.reasoning_effort,
                            speed: request.speed,
                            request_parameters: request
                                .request_parameters
                                .iter()
                                .map(|(k, v)| (k.clone(), v.clone().into()))
                                .collect(),
                        },
                    },
                    Some(true),
                )
            })
            .flatten();
        let detail = TrajectoryReadModelRequestResponse {
            identity: capture.identity.clone(),
            request: ModelRequestTrace {
                model_request_id: Uuid::new_v4().to_string(),
                ordinal,
                purpose: match mode {
                    KernelObservedPurpose::Working => ModelRequestPurpose::Working,
                    KernelObservedPurpose::FinalAnswer => ModelRequestPurpose::FinalAnswer,
                },
                connection_id: connection_id.into(),
                model_id: request.model.clone(),
                call_records: vec![ModelCallRecord {
                    call_record_id: Uuid::new_v4().to_string(),
                    ordinal: 1,
                    observed_state: ModelCallState::InProgress,
                    metrics: Some(LlmCallMetrics {
                        segment_id: capture.clock.segment_id.clone(),
                        started_at_ms: Some(capture.now()),
                        finished_at_ms: None,
                        execution_duration_ms: None,
                        first_output_at_ms: None,
                        last_output_at_ms: None,
                        ttft_ms: None,
                        generation_ms: None,
                        usage: None,
                    }),
                    finish_reason: None,
                    error: None,
                }],
            },
            prompt,
            tools,
            parameters,
            outputs: vec![],
        };
        let absent = capture
            .tools
            .try_lock()
            .ok()
            .map(|tools| {
                tools
                    .iter()
                    .filter(|(id, tool)| {
                        tool.prepared_result.is_some()
                            && !request.messages.iter().any(|m| {
                                m.role == Role::Tool && m.tool_call_id.as_ref() == Some(id)
                            })
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for call_id in absent {
            capture.update_tool(&call_id, |tool| {
                tool.usages
                    .get_or_insert_with(Vec::new)
                    .push(ToolInputUsage {
                        model_request_id: detail.request.model_request_id.clone(),
                        message_index: None,
                        usage: ToolInputUsageKind::NotIncluded,
                    });
            });
        }
        for (index, message) in request.messages.iter().enumerate() {
            if message.role == Role::Tool {
                if let Some(call_id) = &message.tool_call_id {
                    capture.consume_tool(
                        call_id,
                        &message.content,
                        message.attachments.is_none() && message.image_file_ids.is_empty(),
                        index as u32,
                        &detail.request.model_request_id,
                    );
                }
            }
        }
        Some(Arc::new(ModelCapture::new(capture.clone(), detail)))
    }
}
impl Capture {
    fn update_tool(&self, call_id: &str, update: impl FnOnce(&mut TrajectoryReadToolResponse)) {
        if call_id.is_empty() {
            return;
        }
        let Ok(mut tools) = self.tools.try_lock() else {
            return;
        };
        if !tools.contains_key(call_id) && tools.len() >= 128 {
            return;
        }
        let tool = tools
            .entry(call_id.into())
            .or_insert_with(|| TrajectoryReadToolResponse {
                identity: self.identity.clone(),
                tool: TrajectoryTool {
                    call_id: call_id.into(),
                    source_model_request_id: None,
                    source_call_record_id: None,
                },
                prepared_result: None,
                usages: Some(vec![]),
            });
        update(tool);
    }
    fn link_tool(&self, call_id: &str, model_request_id: &str, call_record_id: &str) {
        self.update_tool(call_id, |tool| {
            tool.tool.source_model_request_id = Some(model_request_id.into());
            tool.tool.source_call_record_id = Some(call_record_id.into());
        });
    }
    fn prepare_tool(&self, call_id: &str, message: &ChatMessage) {
        if message.content.len() > 4 * 1024 * 1024 {
            return;
        }
        let Some(reference) = self.payload(
            TrajectoryPayload::PreparedResult {
                message: snapshot_message(message, 0),
            },
            Some(true),
        ) else {
            return;
        };
        self.update_tool(call_id, |tool| {
            tool.prepared_result = Some(reference);
        });
    }
    fn consume_tool(
        &self,
        call_id: &str,
        text: &str,
        text_only: bool,
        index: u32,
        model_request_id: &str,
    ) {
        self.update_tool(call_id, |tool| {
            let usage = if text_only {
                tool.prepared_result
                    .as_ref()
                    .and_then(|reference| self.prepared_equals(&reference.payload_id, text))
                    .map(|equal| {
                        if equal {
                            ToolInputUsageKind::Direct
                        } else {
                            ToolInputUsageKind::Transformed
                        }
                    })
                    .unwrap_or(ToolInputUsageKind::Unknown)
            } else {
                ToolInputUsageKind::Unknown
            };
            let usages = tool.usages.get_or_insert_with(Vec::new);
            if !usages
                .iter()
                .any(|u| u.model_request_id == model_request_id)
            {
                usages.push(ToolInputUsage {
                    model_request_id: model_request_id.into(),
                    message_index: Some(index),
                    usage,
                });
            }
        });
    }
}

/// Counts serialized bytes without retaining bodies; refuses oversized inputs early.
fn bounded_json_size(value: &impl serde::Serialize, limit: usize) -> Option<usize> {
    struct Counter {
        bytes: usize,
        limit: usize,
    }
    impl std::io::Write for Counter {
        fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
            self.bytes = self.bytes.saturating_add(value.len());
            if self.bytes > self.limit {
                return Err(std::io::Error::other("trajectory budget"));
            }
            Ok(value.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { bytes: 0, limit };
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.bytes)
}

#[cfg(test)]
#[path = "trajectory_tests.rs"]
mod tests;
