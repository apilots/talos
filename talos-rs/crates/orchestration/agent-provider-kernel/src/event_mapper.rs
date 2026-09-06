use std::collections::HashMap;

use talos_agent::{AgentEventSink, AgentMessageBlockHandle, AgentMessageHandle, AgentResult};
use talos_agent_api::{
    AgentAssistantText, AgentAssistantTextPhase, AgentContextCompaction,
    AgentContextCompactionDelta, AgentExecutionSummary, AgentMessageBlockContent,
    AgentMessageBlockDeltaContent, AgentMessageBlockStatus, AgentMessageRole, AgentMessageStatus,
    AgentReasoning, AgentRunMetrics, AgentSystemNotice, AgentToolCallArgumentsDelta,
};
use talos_agent_kernel::{
    AgentKernelEvent, AgentKernelEventPayload, AssistantTextPhase, KernelStreamItemKey,
    ToolProgressKind, ToolStreamKey,
};
use talos_error::TalosError;
use talos_types::{TalosTimelineNoticeLevel, TalosToolProgressKind};

mod settlement;
mod tools;

#[derive(Clone)]
struct AssistantBlock {
    handle: AgentMessageBlockHandle,
    kernel_completed: bool,
    phase: AgentAssistantTextPhase,
}

#[derive(Clone)]
struct ToolBlock {
    handle: AgentMessageBlockHandle,
    call_id: String,
}

/// Stateful, single-owner projection from Kernel events into one Agent sink.
pub(crate) struct KernelAgentEventMapper {
    sink: AgentEventSink,
    agent_message: Option<AgentMessageHandle>,
    reasoning: Option<AgentMessageBlockHandle>,
    assistant: HashMap<KernelStreamItemKey, AssistantBlock>,
    tools_by_stream: HashMap<ToolStreamKey, ToolBlock>,
    tools_by_call: HashMap<String, ToolBlock>,
    pending_tool_arguments: HashMap<ToolStreamKey, String>,
    compaction: Option<AgentMessageBlockHandle>,
}

impl KernelAgentEventMapper {
    pub(crate) fn new(sink: AgentEventSink) -> Self {
        Self {
            sink,
            agent_message: None,
            reasoning: None,
            assistant: HashMap::new(),
            tools_by_stream: HashMap::new(),
            tools_by_call: HashMap::new(),
            pending_tool_arguments: HashMap::new(),
            compaction: None,
        }
    }

    pub(crate) fn resumed(
        sink: AgentEventSink,
        message_id: &str,
        block_id: &str,
        call_id: &str,
    ) -> AgentResult<Self> {
        let message = sink.resume_message(message_id)?;
        let block = sink.resume_block(message_id, block_id)?;
        let tool = ToolBlock {
            handle: block,
            call_id: call_id.to_string(),
        };
        Ok(Self {
            sink,
            agent_message: Some(message),
            reasoning: None,
            assistant: HashMap::new(),
            tools_by_stream: HashMap::new(),
            tools_by_call: HashMap::from([(call_id.to_string(), tool)]),
            pending_tool_arguments: HashMap::new(),
            compaction: None,
        })
    }

    pub(crate) async fn map(&mut self, event: AgentKernelEvent) -> AgentResult<()> {
        match event.payload {
            AgentKernelEventPayload::UserInput(_) => {
                // Kernel consumes live input at a model boundary. Keep the next response in
                // a new message so canonical history cannot place it before the User's steer.
                self.close_reasoning().await?;
                if let Some(message) = self.agent_message.take() {
                    self.sink
                        .complete_message(&message, AgentMessageStatus::Completed)
                        .await?;
                }
                Ok(())
            }
            AgentKernelEventPayload::TurnMetrics(data) => {
                let mut metrics = map_metrics(data.metrics);
                metrics.usage = data.usage;
                metrics.context = data.context;
                self.sink.update_metrics(metrics).await
            }
            AgentKernelEventPayload::ThinkingDelta(data) => {
                let block = match &self.reasoning {
                    Some(block) => block.clone(),
                    None => {
                        let message = self.agent_message().await?;
                        let block = self
                            .sink
                            .start_block(
                                &message,
                                AgentMessageBlockContent::Reasoning(AgentReasoning {
                                    text: String::new(),
                                }),
                            )
                            .await?;
                        self.reasoning = Some(block.clone());
                        block
                    }
                };
                self.sink
                    .append_block_delta(&block, AgentMessageBlockDeltaContent::Text(data.delta))
                    .await
            }
            AgentKernelEventPayload::AssistantTextStarted(data) => {
                self.close_reasoning().await?;
                if self.assistant.contains_key(&data.item_key) {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "Kernel assistant block started twice",
                    ));
                }
                let message = self.agent_message().await?;
                let phase = map_assistant_phase(data.phase);
                let block = self
                    .sink
                    .start_block(
                        &message,
                        AgentMessageBlockContent::AssistantText(AgentAssistantText {
                            phase,
                            text: String::new(),
                        }),
                    )
                    .await?;
                self.assistant.insert(
                    data.item_key,
                    AssistantBlock {
                        handle: block,
                        kernel_completed: false,
                        phase,
                    },
                );
                Ok(())
            }
            AgentKernelEventPayload::AssistantTextDelta(data) => {
                let block = self.assistant_block(&data.item_key)?.handle.clone();
                self.sink
                    .append_block_delta(&block, AgentMessageBlockDeltaContent::Text(data.delta))
                    .await
            }
            AgentKernelEventPayload::AssistantTextCompleted(data) => {
                self.assistant_block_mut(&data.item_key)?.kernel_completed = true;
                Ok(())
            }
            AgentKernelEventPayload::AssistantTextCommitted(data) => {
                let committed_phase = map_assistant_phase(data.phase);
                let assistant = self.assistant_block(&data.item_key)?.clone();
                let block = assistant.handle;
                if assistant.phase == AgentAssistantTextPhase::Pending {
                    self.sink
                        .append_block_delta(
                            &block,
                            AgentMessageBlockDeltaContent::AssistantTextPhase(committed_phase),
                        )
                        .await?;
                } else if assistant.phase != committed_phase {
                    return Err(TalosError::new(
                        talos_error::ErrorCode::Conflict,
                        "Kernel committed a conflicting assistant text phase",
                    ));
                }
                if self.assistant_block(&data.item_key)?.kernel_completed {
                    self.sink
                        .complete_block(&block, AgentMessageBlockStatus::Completed)
                        .await?;
                    self.assistant.remove(&data.item_key);
                }
                Ok(())
            }
            AgentKernelEventPayload::ExecutionSummary(data) => {
                self.close_reasoning().await?;
                let message = self.agent_message().await?;
                let block = self
                    .sink
                    .start_block(
                        &message,
                        AgentMessageBlockContent::ExecutionSummary(AgentExecutionSummary {
                            text: data.text,
                        }),
                    )
                    .await?;
                self.sink
                    .complete_block(&block, AgentMessageBlockStatus::Completed)
                    .await
            }
            AgentKernelEventPayload::ToolArgsDelta(data) => {
                if let Some(tool) = self.tools_by_stream.get(&data.stream_key) {
                    return self
                        .sink
                        .append_block_delta(
                            &tool.handle,
                            AgentMessageBlockDeltaContent::ToolCallArguments(
                                AgentToolCallArgumentsDelta {
                                    json_fragment: data.delta,
                                },
                            ),
                        )
                        .await;
                }
                self.pending_tool_arguments
                    .entry(data.stream_key)
                    .or_default()
                    .push_str(&data.delta);
                Ok(())
            }
            AgentKernelEventPayload::ToolCallStart(data) => {
                let stream_key = ToolStreamKey {
                    round: data.round,
                    index: data.index,
                };
                self.start_tool(
                    stream_key,
                    data.call_id,
                    data.tool_name,
                    data.arguments.into(),
                )
                .await
                .map(|_| ())
            }
            AgentKernelEventPayload::ToolQueued(data) => self
                .ensure_tool(data.stream_key, data.call_id, data.tool_name)
                .await
                .map(|_| ()),
            AgentKernelEventPayload::ToolExecuting(data) => {
                let tool = self
                    .ensure_tool(data.stream_key, data.call_id, data.tool_name.clone())
                    .await?;
                self.append_tool_progress(
                    &tool,
                    TalosToolProgressKind::Status,
                    format!("Executing {}", data.tool_name),
                    event.occurred_at,
                )
                .await
            }
            AgentKernelEventPayload::ToolProgressDelta(data) => {
                let tool = self
                    .ensure_tool(data.stream_key, data.call_id, data.tool_name)
                    .await?;
                self.append_tool_progress(
                    &tool,
                    map_tool_progress_kind(data.progress_kind),
                    data.message,
                    event.occurred_at,
                )
                .await
            }
            AgentKernelEventPayload::ToolResult(data) => self.map_tool_result(data).await,
            AgentKernelEventPayload::UserActionRequest(_) => self.close_reasoning().await,
            AgentKernelEventPayload::PromptWarning(data) => {
                self.map_notice(warning_message(data.warning)).await
            }
            AgentKernelEventPayload::ContextCompaction(data) => self.map_compaction(data).await,
        }
    }

    pub(crate) fn waiting_handles(&self, call_id: &str) -> AgentResult<(&str, &str)> {
        let message = self.agent_message.as_ref().ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Kernel waiting result has no Agent message",
            )
        })?;
        let tool = self.tools_by_call.get(call_id).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Kernel waiting result has no Tool block",
            )
        })?;
        Ok((message.message_id(), tool.handle.block_id()))
    }

    async fn agent_message(&mut self) -> AgentResult<AgentMessageHandle> {
        if let Some(message) = &self.agent_message {
            return Ok(message.clone());
        }
        let message = self.sink.start_message(AgentMessageRole::Agent).await?;
        self.agent_message = Some(message.clone());
        Ok(message)
    }

    async fn close_reasoning(&mut self) -> AgentResult<()> {
        self.close_reasoning_with(AgentMessageBlockStatus::Completed)
            .await
    }

    async fn close_reasoning_with(&mut self, status: AgentMessageBlockStatus) -> AgentResult<()> {
        if let Some(block) = self.reasoning.clone() {
            self.sink.complete_block(&block, status).await?;
            self.reasoning = None;
        }
        Ok(())
    }

    fn assistant_block(&self, key: &KernelStreamItemKey) -> AgentResult<&AssistantBlock> {
        self.assistant.get(key).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Kernel assistant block does not exist",
            )
        })
    }

    fn assistant_block_mut(
        &mut self,
        key: &KernelStreamItemKey,
    ) -> AgentResult<&mut AssistantBlock> {
        self.assistant.get_mut(key).ok_or_else(|| {
            TalosError::new(
                talos_error::ErrorCode::Conflict,
                "Kernel assistant block does not exist",
            )
        })
    }

    async fn map_notice(&self, message_text: String) -> AgentResult<()> {
        let message = self.sink.start_message(AgentMessageRole::System).await?;
        let block = self
            .sink
            .start_block(
                &message,
                AgentMessageBlockContent::SystemNotice(AgentSystemNotice {
                    level: TalosTimelineNoticeLevel::Warning,
                    content: talos_agent_api::SystemNoticeContent::Text(message_text),
                }),
            )
            .await?;
        self.sink
            .complete_block(&block, AgentMessageBlockStatus::Completed)
            .await?;
        self.sink
            .complete_message(&message, AgentMessageStatus::Completed)
            .await
    }

    async fn map_compaction(
        &mut self,
        data: talos_agent_kernel::ContextCompactionStreamData,
    ) -> AgentResult<()> {
        let block = match &self.compaction {
            Some(block) => block.clone(),
            None => {
                let message = self.agent_message().await?;
                let block = self
                    .sink
                    .start_block(
                        &message,
                        AgentMessageBlockContent::ContextCompaction(AgentContextCompaction {
                            trigger: data.trigger,
                            stage: data.stage,
                            outcome: None,
                            metrics: None,
                            message: None,
                        }),
                    )
                    .await?;
                self.compaction = Some(block.clone());
                block
            }
        };
        self.sink
            .append_block_delta(
                &block,
                AgentMessageBlockDeltaContent::ContextCompaction(AgentContextCompactionDelta {
                    stage: Some(data.stage),
                    outcome: data.outcome,
                    metrics: data.metrics,
                    message: data.message,
                }),
            )
            .await
    }
}

fn map_assistant_phase(phase: AssistantTextPhase) -> AgentAssistantTextPhase {
    match phase {
        AssistantTextPhase::Unknown => AgentAssistantTextPhase::Pending,
        AssistantTextPhase::Commentary => AgentAssistantTextPhase::Commentary,
        AssistantTextPhase::FinalAnswer => AgentAssistantTextPhase::FinalAnswer,
    }
}

fn map_metrics(metrics: talos_types::TurnExecutionMetrics) -> AgentRunMetrics {
    AgentRunMetrics {
        model_rounds: metrics.model_rounds,
        tool_calls: metrics.tool_calls,
        tool_successes: metrics.tool_successes,
        tool_errors: metrics.tool_errors,
        usage: metrics.usage,
        context: None,
        duration_ms: metrics.duration_ms,
    }
}

fn map_tool_progress_kind(kind: ToolProgressKind) -> TalosToolProgressKind {
    match kind {
        ToolProgressKind::Status => TalosToolProgressKind::Status,
        ToolProgressKind::Stdout => TalosToolProgressKind::Stdout,
        ToolProgressKind::Stderr => TalosToolProgressKind::Stderr,
    }
}

fn warning_message(warning: talos_agent_kernel::KernelWarning) -> String {
    match warning {
        talos_agent_kernel::KernelWarning::PromptBootstrapFileMissing(data) => {
            format!("Prompt bootstrap file is missing: {:?}", data.file_kind)
        }
        talos_agent_kernel::KernelWarning::PromptBootstrapFileTruncated(data) => format!(
            "Prompt bootstrap file was truncated: {:?} ({} of {} characters)",
            data.file_kind, data.injected_chars, data.raw_chars
        ),
        talos_agent_kernel::KernelWarning::PromptBootstrapFileDropped(data) => {
            format!("Prompt bootstrap file was dropped: {:?}", data.file_kind)
        }
        talos_agent_kernel::KernelWarning::PromptDynamicSectionDropped(data) => format!(
            "Prompt dynamic sections were dropped: {}",
            data.sections
                .iter()
                .map(|section| format!("{section:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
#[path = "event_mapper_tests.rs"]
mod tests;
