use crate::{
    warnings::derive_prompt_warnings, InvocationContext, KernelEvent, KernelEventType,
    KernelStreamEvent, KernelWarning,
};
use chrono::Utc;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::sync::Arc;
use talos_config::AgentKernelConfig;
use talos_error::TalosResult;
use talos_llm_provider::{
    ChatMessage, ChatRequest, FunctionCall, LlmProvider, ToolChoice, ToolStreamEvent,
};
use talos_memory::{
    Memory, MemoryGetResult, MemorySearchResponse, MemoryStore, RecordHeader, RetrievedMemoryItem,
    SessionPromptContextConfig, SummaryRecord, TalosMessage, TalosMessageRole, TalosTurn,
    ToolCallRecord, ToolResultRecord,
};
use talos_prompt::{
    render_prompt_assembly, PromptBudget, PromptBuildConfig, PromptBuilder, PromptDiagnostics,
    PromptLoadPolicy, PromptPhase,
};
use talos_tools::ToolRegistry;
use talos_types::talos::{TalosContentItem, TalosOutputItem, TalosUsage};
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct GeneralTurnResult {
    pub output_text: String,
    pub output_items: Vec<TalosOutputItem>,
    pub stop_reason: String,
    pub usage: TalosUsage,
    pub prompt_diagnostics: PromptDiagnostics,
    pub warnings: Vec<KernelWarning>,
}

pub async fn flush_session_memory(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    enforce_threshold: bool,
    persist_session_summary: bool,
    flush_workspace_memory: bool,
) -> TalosResult<()> {
    if persist_session_summary || flush_workspace_memory {
        if enforce_threshold {
            let threshold = kernel_config.memory.compaction_threshold_bytes as u64;
            if threshold == 0 {
                return Ok(());
            }
            let session_bytes = memory_store.session_size_bytes(session_id).await?;
            if session_bytes < threshold {
                return Ok(());
            }
        }
    } else {
        return Ok(());
    }

    let context = memory_store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                recent_messages_limit: 48,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await?;
    if context.recent_messages.is_empty() {
        return Ok(());
    }
    let transcript = context
        .recent_messages
        .iter()
        .map(|message| match message.role {
            TalosMessageRole::User => format!("User: {}", message.text.trim()),
            TalosMessageRole::Assistant => format!("Assistant: {}", message.text.trim()),
            TalosMessageRole::System => format!("System: {}", message.text.trim()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        return Ok(());
    }

    let selected_model = selected_model.to_string();
    let llm_provider_for_summary = llm_provider.clone();
    let request = ChatRequest::builder()
        .model(selected_model)
        .messages(vec![
            ChatMessage::system(
                "Summarize the conversation into durable facts, user preferences, open tasks, and \
                 unresolved risks. Keep under 220 words.",
            ),
            ChatMessage::user(format!("Conversation transcript:\n{}", transcript)),
        ])
        .build();
    let response = llm_provider_for_summary.chat(request).await?;
    let summary = response.content.trim().to_string();
    if summary.is_empty() {
        return Ok(());
    }
    let latest_seq = memory_store.latest_seq(session_id).await?;
    let source_turn_count = context
        .recent_messages
        .iter()
        .filter_map(|item| item.header.turn_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if persist_session_summary {
        memory_store
            .save_summary(&SummaryRecord {
                version: 2,
                header: RecordHeader {
                    id: Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    turn_id: None,
                    timestamp: Utc::now(),
                },
                covers_until_seq: latest_seq,
                source_turn_count,
                content: summary.clone(),
            })
            .await?;
    }

    if flush_workspace_memory {
        memory_store
            .append_workspace_memory(&talos_memory::WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                title: Some("Durable Memory".to_string()),
                content: summary.clone(),
            })
            .await?;
    }

    Ok(())
}

async fn compact_workspace_context_if_needed(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<()> {
    flush_session_memory(
        session_id,
        selected_model,
        llm_provider,
        memory_store,
        kernel_config,
        true,
        true,
        kernel_config.memory.workspace_memory_flush_enabled
            && kernel_config.memory.workspace_memory_flush_on_compaction,
    )
    .await
}

fn build_reasoning_summary(tool_call_count: usize, tool_error_count: usize) -> String {
    if tool_call_count == 0 {
        "Kernel summary: answered directly without invoking tools.".to_string()
    } else if tool_error_count == 0 {
        format!(
            "Kernel summary: executed {} tool call(s) successfully before final response.",
            tool_call_count
        )
    } else {
        format!(
            "Kernel summary: executed {} tool call(s), {} call(s) returned errors; final response generated with available results.",
            tool_call_count, tool_error_count
        )
    }
}

#[derive(Debug, Default)]
struct PendingToolCall {
    id: Option<String>,
    name: Option<String>,
    arguments_buffer: String,
    final_arguments: Option<Value>,
}

#[derive(Debug, Default)]
struct StreamRound {
    content: String,
    reasoning: String,
    tool_calls: Vec<FunctionCall>,
    finish_reason: Option<String>,
    saw_output_delta: bool,
    usage: TalosUsage,
}

#[derive(Debug, Clone)]
struct ToolResultLog {
    call_id: String,
    tool: String,
    payload: Value,
    is_error: bool,
    truncated: bool,
    size_bytes: usize,
}

fn parse_retrieved_memory_items(tool_name: &str, payload: &Value) -> Vec<RetrievedMemoryItem> {
    match tool_name {
        "memory_search" => match serde_json::from_value::<MemorySearchResponse>(payload.clone()) {
            Ok(response) => response.into_retrieved_items(),
            Err(err) => {
                warn!(
                    tool_name = %tool_name,
                    error = %err,
                    payload = %payload,
                    "failed to parse memory_search payload"
                );
                Vec::new()
            }
        },
        "memory_get" => match serde_json::from_value::<MemoryGetResult>(payload.clone()) {
            Ok(result) if !result.text.trim().is_empty() => vec![RetrievedMemoryItem {
                path: result.path,
                snippet: result.text.trim().to_string(),
                citation: None,
                score: 1.0,
            }],
            Ok(_) => Vec::new(),
            Err(err) => {
                warn!(
                    tool_name = %tool_name,
                    error = %err,
                    payload = %payload,
                    "failed to parse memory_get payload"
                );
                Vec::new()
            }
        },
        _ => Vec::new(),
    }
}

fn build_retrieved_memories_message(
    items: &[RetrievedMemoryItem],
    max_items: usize,
    max_chars: usize,
) -> Option<ChatMessage> {
    if items.is_empty() || max_items == 0 || max_chars == 0 {
        return None;
    }
    let mut content = String::new();
    for item in items.iter().take(max_items) {
        let line = format!(
            "- {}{}: {}",
            item.path,
            item.citation
                .as_ref()
                .map(|citation| format!(" ({citation})"))
                .unwrap_or_default(),
            item.snippet.trim()
        );
        if content.len() + line.len() + 1 > max_chars {
            break;
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&line);
    }
    if content.is_empty() {
        None
    } else {
        Some(ChatMessage::system(format!(
            "Retrieved memories:\n{}",
            content
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ContextStateBags, InvocationLimits, TurnPhase};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use talos_llm_provider::{
        ChatChunk, ChatRequest, ChatResponse, ChatStream, LlmProvider, Usage,
    };
    use talos_tools::ToolRegistry;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    struct NoopLlmProvider;

    #[async_trait]
    impl LlmProvider for NoopLlmProvider {
        fn name(&self) -> &str {
            "noop"
        }

        async fn chat(&self, _request: ChatRequest) -> TalosResult<ChatResponse> {
            Ok(ChatResponse {
                content: "ok".to_string(),
                model: "noop".to_string(),
                usage: Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    total_tokens: 2,
                },
                finish_reason: Some("stop".to_string()),
                tool_calls: None,
            })
        }

        async fn stream(&self, _request: ChatRequest) -> TalosResult<ChatStream> {
            let (_tx, rx) = mpsc::channel::<TalosResult<ChatChunk>>(1);
            Ok(ChatStream::new(rx))
        }

        fn default_model(&self) -> &str {
            "noop"
        }
    }

    fn build_ctx_with_stream(
        stream_tx: Option<mpsc::Sender<crate::KernelStreamEvent>>,
    ) -> InvocationContext {
        InvocationContext {
            session_id: "s1".to_string(),
            invocation_id: "i1".to_string(),
            agent_id: "a1".to_string(),
            workspace_id: "w1".to_string(),
            input: "hello".to_string(),
            model: "m1".to_string(),
            phase: TurnPhase::Execution,
            planning_instruction: None,
            transfer_to: None,
            state: HashMap::new(),
            state_bags: ContextStateBags::default(),
            limits: InvocationLimits::default(),
            events: Vec::new(),
            stream_tx,
        }
    }

    async fn seed_recent_user_message(memory_store: &MemoryStore) {
        let timestamp = Utc::now();
        memory_store
            .append_turn(&TalosTurn {
                turn_id: "turn-1".to_string(),
                session_id: "s1".to_string(),
                seq: memory_store
                    .next_turn_seq("s1")
                    .await
                    .expect("seq should load"),
                timestamp,
                messages: vec![TalosMessage {
                    header: RecordHeader {
                        id: "turn-1:u".to_string(),
                        session_id: "s1".to_string(),
                        turn_id: Some("turn-1".to_string()),
                        timestamp,
                    },
                    role: TalosMessageRole::User,
                    text: "history that should be dropped".to_string(),
                }],
                reasoning: Vec::new(),
                tool_calls: Vec::new(),
                tool_results: Vec::new(),
            })
            .await
            .expect("append should succeed");
    }

    #[test]
    fn test_parse_retrieved_memory_items_from_memory_search() {
        let payload = json!(MemorySearchResponse {
            results: vec![talos_memory::MemorySearchResult {
                path: "memory/2026-03-24.md".to_string(),
                snippet: "launch on friday".to_string(),
                citation: Some("memory/2026-03-24.md#L1-L1".to_string()),
                score: 0.9,
                start_line: 1,
                end_line: 1,
            }],
        });

        let items = parse_retrieved_memory_items("memory_search", &payload);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "memory/2026-03-24.md");
        assert_eq!(items[0].score, 0.9);
    }

    #[test]
    fn test_parse_retrieved_memory_items_ignores_invalid_search_rows() {
        let payload = json!({
            "results": [
                {
                    "path": "memory/2026-03-25.md",
                    "snippet": "missing score"
                }
            ]
        });

        let items = parse_retrieved_memory_items("memory_search", &payload);
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_retrieved_memory_items_from_memory_get() {
        let payload = json!(MemoryGetResult {
            path: "MEMORY.md".to_string(),
            text: "remember tea preference".to_string(),
            from: 1,
            lines: 5,
        });

        let items = parse_retrieved_memory_items("memory_get", &payload);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].path, "MEMORY.md");
    }

    #[tokio::test]
    async fn test_run_general_turn_emits_prompt_warning_event() {
        let provider = Arc::new(NoopLlmProvider);
        let tool_registry = Arc::new(ToolRegistry::new());
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        seed_recent_user_message(memory_store.as_ref()).await;
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.prompt.max_dynamic_tokens = 1;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let mut ctx = build_ctx_with_stream(None);

        let result = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "hello",
            provider,
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        assert!(!result.warnings.is_empty());
        assert!(ctx.events.iter().any(|event| {
            matches!(event.event_type, KernelEventType::PromptWarning)
                && event.message == "prompt_warning"
        }));
    }

    #[tokio::test]
    async fn test_run_general_turn_streams_prompt_warning_event() {
        let provider = Arc::new(NoopLlmProvider);
        let tool_registry = Arc::new(ToolRegistry::new());
        let workspace_dir = std::env::temp_dir().join(Uuid::new_v4().to_string());
        let memory_store = Arc::new(MemoryStore::new(workspace_dir.clone()));
        seed_recent_user_message(memory_store.as_ref()).await;
        let mut config = AgentKernelConfig::default();
        config.prompt.bootstrap.enabled = false;
        config.prompt.max_dynamic_tokens = 1;
        config.memory.workspace_dir = workspace_dir.to_string_lossy().to_string();
        let (tx, mut rx) = mpsc::channel(16);
        let mut ctx = build_ctx_with_stream(Some(tx));

        let _ = run_general_turn(
            &mut ctx,
            "talos",
            "assistant",
            "hello",
            provider,
            tool_registry,
            memory_store,
            &config,
        )
        .await
        .expect("general turn should succeed");

        let mut saw_prompt_warning = false;
        while let Ok(event) = rx.try_recv() {
            if event.event_type == "prompt.warning" {
                saw_prompt_warning = true;
                break;
            }
        }
        assert!(saw_prompt_warning, "stream should include prompt.warning");
    }
}

async fn emit_stream_event(
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
    event_type: &str,
    data: Value,
) {
    if let Some(tx) = stream_tx {
        let _ = tx.send(KernelStreamEvent::new(event_type, data)).await;
    }
}

async fn emit_prompt_warning_events(ctx: &mut InvocationContext, warnings: &[KernelWarning]) {
    for warning in warnings {
        let payload = json!({
            "warning": warning,
            "agent_id": ctx.agent_id,
            "session_id": ctx.session_id,
            "phase": ctx.phase,
        });
        ctx.events.push(KernelEvent::new(
            KernelEventType::PromptWarning,
            "prompt_warning",
            Some(payload.clone()),
        ));
        emit_stream_event(ctx.stream_tx.as_ref(), "prompt.warning", payload).await;
    }
}

fn finalize_tool_calls(
    round: u32,
    pending_calls: BTreeMap<usize, PendingToolCall>,
) -> Vec<FunctionCall> {
    let mut calls = Vec::new();

    for (index, call) in pending_calls {
        let Some(name) = call.name.filter(|value| !value.trim().is_empty()) else {
            continue;
        };

        let id = call
            .id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("tool-{}-{}", round, index));
        let arguments = if let Some(value) = call.final_arguments {
            value
        } else if call.arguments_buffer.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_json::from_str::<Value>(&call.arguments_buffer)
                .unwrap_or_else(|_| Value::String(call.arguments_buffer))
        };

        calls.push(FunctionCall::new(id, name, arguments));
    }

    calls
}

async fn stream_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
    round: u32,
    stream_tx: Option<&mpsc::Sender<KernelStreamEvent>>,
) -> TalosResult<StreamRound> {
    let mut stream = llm_provider.stream_with_tools(request).await?;
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut finish_reason = None;
    let mut pending_calls: BTreeMap<usize, PendingToolCall> = BTreeMap::new();
    let mut saw_output_delta = false;
    let mut usage = TalosUsage::default();

    while let Some(event) = stream.next().await {
        match event? {
            ToolStreamEvent::TextDelta { delta } => {
                content.push_str(&delta);
                saw_output_delta = true;
                emit_stream_event(
                    stream_tx,
                    "output_delta",
                    json!({
                        "delta": delta,
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::ReasoningDelta { delta } => {
                reasoning.push_str(&delta);
                emit_stream_event(
                    stream_tx,
                    "reasoning_delta",
                    json!({
                        "delta": delta,
                        "source": "provider",
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::ToolCallStart { index, id, name } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
            }
            ToolStreamEvent::ToolCallDelta {
                index,
                arguments_delta,
            } => {
                let entry = pending_calls.entry(index).or_default();
                entry.arguments_buffer.push_str(&arguments_delta);
            }
            ToolStreamEvent::ToolCallEnd {
                index,
                id,
                name,
                arguments,
            } => {
                let entry = pending_calls.entry(index).or_default();
                if id.is_some() {
                    entry.id = id;
                }
                if name.is_some() {
                    entry.name = name;
                }
                let arguments_copy = arguments.clone();
                entry.final_arguments = Some(arguments);
                emit_stream_event(
                    stream_tx,
                    "tool_call_start",
                    json!({
                        "index": index,
                        "id": entry.id.clone(),
                        "name": entry.name.clone(),
                        "arguments": arguments_copy,
                        "round": round,
                    }),
                )
                .await;
            }
            ToolStreamEvent::Done {
                finish_reason: final_reason,
                usage: final_usage,
            } => {
                finish_reason = final_reason;
                if let Some(final_usage) = final_usage {
                    usage = TalosUsage {
                        input_tokens: final_usage.prompt_tokens,
                        output_tokens: final_usage.completion_tokens,
                        total_tokens: final_usage.total_tokens,
                    };
                }
                break;
            }
        }
    }

    Ok(StreamRound {
        content,
        reasoning,
        tool_calls: finalize_tool_calls(round, pending_calls),
        finish_reason,
        saw_output_delta,
        usage,
    })
}

async fn chat_round(
    llm_provider: Arc<dyn LlmProvider>,
    request: ChatRequest,
) -> TalosResult<StreamRound> {
    let response = llm_provider.chat_with_tools(request).await?;
    let tool_calls = response.get_tool_calls().to_vec();
    Ok(StreamRound {
        content: response.content,
        reasoning: String::new(),
        tool_calls,
        finish_reason: response.finish_reason,
        saw_output_delta: false,
        usage: TalosUsage {
            input_tokens: response.usage.prompt_tokens,
            output_tokens: response.usage.completion_tokens,
            total_tokens: response.usage.total_tokens,
        },
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn run_general_turn(
    ctx: &mut InvocationContext,
    agent_name: &str,
    agent_description: &str,
    input: &str,
    llm_provider: Arc<dyn LlmProvider>,
    tool_registry: Arc<ToolRegistry>,
    memory_store: Arc<MemoryStore>,
    kernel_config: &AgentKernelConfig,
) -> TalosResult<GeneralTurnResult> {
    let session_id = ctx.session_id.clone();
    let agent_id = ctx.agent_id.clone();
    let selected_model = ctx.model.clone();
    let stream_tx = ctx.stream_tx.clone();

    let phase = match ctx.phase {
        crate::TurnPhase::Planning => PromptPhase::Planning,
        crate::TurnPhase::Execution => PromptPhase::Execution,
    };

    let prompt_builder = PromptBuilder::new()
        .agent(agent_name.to_string(), agent_description.to_string())
        .phase(phase)
        .planner_mode(kernel_config.planner_mode.clone())?
        .planning_instruction(ctx.planning_instruction.clone())
        .config(PromptBuildConfig {
            budget: PromptBudget {
                max_static_tokens: kernel_config.prompt.max_static_tokens,
                max_dynamic_tokens: kernel_config.prompt.max_dynamic_tokens,
            },
            load_policy: PromptLoadPolicy {
                include_session_summary: kernel_config.prompt.include_session_summary,
                include_recent_messages: kernel_config.prompt.include_recent_messages,
                include_retrieved_memories: kernel_config.prompt.include_retrieved_memories,
                session_recent_limit: kernel_config.memory.session_recent_messages_limit,
                retrieved_memory_max_items: kernel_config.prompt.retrieved_memory_max_items,
                retrieved_memory_max_chars: kernel_config.prompt.retrieved_memory_max_chars,
                unified_recent_limit: 10,
            },
            prompt: kernel_config.prompt.clone(),
            memory: kernel_config.memory.clone(),
        })
        .tools(tool_registry.as_ref())
        .memory_store(&session_id, memory_store.as_ref())
        .user_input(input.to_string());
    let prompt_assembly = prompt_builder.assemble().await?;
    let prompt_diagnostics = prompt_assembly.diagnostics.clone();
    let warnings = derive_prompt_warnings(&prompt_diagnostics, kernel_config);
    tracing::debug!(
        session_id = %session_id,
        agent_id = %agent_id,
        prompt_mode = ?prompt_diagnostics.mode,
        included_sections = ?prompt_diagnostics.included_sections,
        dropped_sections = ?prompt_diagnostics.dropped_sections,
        truncated_sections = ?prompt_diagnostics.truncated_sections,
        static_estimated_tokens = prompt_diagnostics.static_estimated_tokens,
        dynamic_estimated_tokens = prompt_diagnostics.dynamic_estimated_tokens,
        total_estimated_tokens = prompt_diagnostics.total_estimated_tokens,
        bootstrap_files = ?prompt_diagnostics.bootstrap_files,
        "prompt diagnostics"
    );
    if !warnings.is_empty() {
        warn!(
            session_id = %session_id,
            agent_id = %agent_id,
            warnings = ?warnings,
            "prompt warnings emitted"
        );
    }
    emit_prompt_warning_events(ctx, &warnings).await;
    let mut messages = render_prompt_assembly(&prompt_assembly);

    let tool_definitions = tool_registry.get_tool_definitions();
    let max_tool_rounds = kernel_config.max_tool_rounds.max(1);

    let mut round = 0_u32;
    let mut output_items: Vec<TalosOutputItem> = Vec::new();
    let mut usage = TalosUsage::default();
    let mut tool_call_count = 0_usize;
    let mut tool_error_count = 0_usize;
    let mut reasoning_log: Vec<String> = Vec::new();
    let mut tool_call_log: Vec<ToolCallRecord> = Vec::new();
    let mut tool_result_log: Vec<ToolResultLog> = Vec::new();
    let mut retrieved_memories: Vec<RetrievedMemoryItem> = Vec::new();
    let mut saw_output_delta = false;

    let (mut final_text, stop_reason) = loop {
        let mut builder = ChatRequest::builder()
            .messages(messages.clone())
            .model(selected_model.clone());

        if !tool_definitions.is_empty() {
            builder = builder
                .tools(tool_definitions.clone())
                .tool_choice(ToolChoice::Auto)
                .parallel_tool_calls(true);
        }

        let round_request = builder.build();
        let round_response = if stream_tx.is_some() {
            stream_round(
                llm_provider.clone(),
                round_request,
                round,
                stream_tx.as_ref(),
            )
            .await?
        } else {
            chat_round(llm_provider.clone(), round_request).await?
        };
        saw_output_delta = saw_output_delta || round_response.saw_output_delta;
        usage.input_tokens = usage
            .input_tokens
            .saturating_add(round_response.usage.input_tokens);
        usage.output_tokens = usage
            .output_tokens
            .saturating_add(round_response.usage.output_tokens);
        usage.total_tokens = usage
            .total_tokens
            .saturating_add(round_response.usage.total_tokens);

        if !round_response.reasoning.trim().is_empty() {
            let reasoning = round_response.reasoning.trim().to_string();
            output_items.push(TalosOutputItem::Reasoning {
                text: reasoning.clone(),
            });
            reasoning_log.push(reasoning);
        }

        if !round_response.tool_calls.is_empty() && !tool_definitions.is_empty() {
            let calls = round_response.tool_calls;
            for call in &calls {
                output_items.push(TalosOutputItem::ToolCall {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
                tool_call_count += 1;
                tool_call_log.push(ToolCallRecord {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                });
            }

            messages.push(ChatMessage::assistant_with_tool_calls(
                round_response.content.clone(),
                calls.clone(),
            ));

            let context = tool_registry.build_context(
                format!("agent-{}", agent_id),
                agent_name.to_string(),
                Some(Arc::new(kernel_config.clone())),
            );

            let mut tool_messages = Vec::new();
            for call in calls {
                info!(
                    agent_id = %agent_id,
                    tool_name = %call.name,
                    call_id = %call.id,
                    arguments = %call.arguments,
                    "executing tool call: {}",
                    call.name
                );

                match tool_registry
                    .execute_with_context(&call.name, call.arguments.clone(), context.clone())
                    .await
                {
                    Ok(tool_result) => {
                        let output_truncated = tool_result.output.contains("[truncated]");
                        let payload = if tool_result.success {
                            serde_json::json!({
                                "ok": true,
                                "tool": call.name,
                                "result": tool_result.output,
                                "truncated": output_truncated,
                            })
                        } else {
                            tool_error_count += 1;
                            let error = tool_result
                                .error
                                .unwrap_or_else(|| "unknown error".to_string());
                            serde_json::json!({
                                "ok": false,
                                "tool": call.name,
                                "error": error.clone(),
                                "truncated": error.contains("[truncated]"),
                            })
                        };

                        let is_error = payload
                            .get("ok")
                            .and_then(|v| v.as_bool())
                            .map(|ok| !ok)
                            .unwrap_or(true);
                        output_items.push(TalosOutputItem::ToolResult {
                            call_id: call.id.clone(),
                            content: vec![TalosContentItem::Json {
                                value: payload.clone(),
                            }],
                            is_error,
                        });
                        emit_stream_event(
                            stream_tx.as_ref(),
                            "tool_result",
                            json!({
                                "call_id": call.id.clone(),
                                "tool": call.name.clone(),
                                "payload": payload.clone(),
                                "is_error": is_error,
                            }),
                        )
                        .await;

                        tool_result_log.push(ToolResultLog {
                            call_id: call.id.clone(),
                            tool: call.name.clone(),
                            payload: payload.clone(),
                            is_error,
                            truncated: payload
                                .get("truncated")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            size_bytes: serde_json::to_vec(&payload)
                                .map(|buf| buf.len())
                                .unwrap_or(0),
                        });
                        retrieved_memories
                            .extend(parse_retrieved_memory_items(&call.name, &payload));

                        tool_messages.push(ChatMessage::tool(
                            call.id.clone(),
                            call.name.clone(),
                            payload.to_string(),
                        ));
                        info!(
                            agent_id = %agent_id,
                            tool_name = %call.name,
                            call_id = %call.id,
                            ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
                            "tool call completed: {}",
                            call.name
                        );
                    }
                    Err(e) => {
                        tool_error_count += 1;
                        let payload = serde_json::json!({
                            "ok": false,
                            "tool": call.name,
                            "error": e.to_string(),
                        });
                        output_items.push(TalosOutputItem::ToolResult {
                            call_id: call.id.clone(),
                            content: vec![TalosContentItem::Json {
                                value: payload.clone(),
                            }],
                            is_error: true,
                        });
                        emit_stream_event(
                            stream_tx.as_ref(),
                            "tool_result",
                            json!({
                                "call_id": call.id.clone(),
                                "tool": call.name.clone(),
                                "payload": payload.clone(),
                                "is_error": true,
                            }),
                        )
                        .await;
                        tool_result_log.push(ToolResultLog {
                            call_id: call.id.clone(),
                            tool: call.name.clone(),
                            payload: payload.clone(),
                            is_error: true,
                            truncated: false,
                            size_bytes: serde_json::to_vec(&payload)
                                .map(|buf| buf.len())
                                .unwrap_or(0),
                        });
                        retrieved_memories
                            .extend(parse_retrieved_memory_items(&call.name, &payload));
                        tool_messages.push(ChatMessage::tool(
                            call.id.clone(),
                            call.name.clone(),
                            payload.to_string(),
                        ));
                        info!(
                            agent_id = %agent_id,
                            tool_name = %call.name,
                            call_id = %call.id,
                            error = %e,
                            "tool call failed: {}",
                            call.name
                        );
                    }
                }
            }

            messages.extend(tool_messages);
            if kernel_config.prompt.include_retrieved_memories {
                if let Some(message) = build_retrieved_memories_message(
                    &retrieved_memories,
                    kernel_config.prompt.retrieved_memory_max_items,
                    kernel_config.prompt.retrieved_memory_max_chars,
                ) {
                    if !messages.iter().rev().take(4).any(|item| {
                        item.role == talos_llm_provider::Role::System
                            && item.content == message.content
                    }) {
                        messages.push(message);
                    }
                }
            }
            round += 1;
            if round >= max_tool_rounds {
                break (
                    "[tool-call rounds exceeded]".to_string(),
                    "tool_round_limit".to_string(),
                );
            }
            continue;
        }

        let final_text = round_response.content.clone();
        let stop_reason = round_response
            .finish_reason
            .clone()
            .unwrap_or_else(|| "end_turn".to_string());
        messages.push(ChatMessage::assistant(round_response.content.clone()));
        break (final_text, stop_reason);
    };

    if final_text.trim().is_empty() {
        final_text = if tool_call_count > 0 {
            "Tool execution completed.".to_string()
        } else {
            "No response generated.".to_string()
        };
    }

    if !saw_output_delta && !final_text.trim().is_empty() {
        emit_stream_event(
            stream_tx.as_ref(),
            "output_delta",
            json!({
                "delta": final_text.clone(),
                "source": "kernel_fallback",
            }),
        )
        .await;
    }

    let reasoning_summary = build_reasoning_summary(tool_call_count, tool_error_count);
    emit_stream_event(
        stream_tx.as_ref(),
        "reasoning_delta",
        json!({
            "delta": reasoning_summary.clone(),
            "source": "kernel",
        }),
    )
    .await;
    output_items.push(TalosOutputItem::Reasoning {
        text: reasoning_summary.clone(),
    });
    output_items.push(TalosOutputItem::OutputText {
        text: final_text.clone(),
    });

    let seq = memory_store.next_turn_seq(&session_id).await?;
    let turn_id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let talos_turn = TalosTurn {
        turn_id: turn_id.clone(),
        session_id: session_id.to_string(),
        seq,
        timestamp: now,
        messages: vec![
            TalosMessage {
                header: RecordHeader {
                    id: format!("{}:u", turn_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                role: TalosMessageRole::User,
                text: input.to_string(),
            },
            TalosMessage {
                header: RecordHeader {
                    id: format!("{}:a", turn_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                role: TalosMessageRole::Assistant,
                text: final_text.clone(),
            },
        ],
        reasoning: {
            let mut values = reasoning_log;
            values.push(reasoning_summary.clone());
            values
        },
        tool_calls: tool_call_log,
        tool_results: tool_result_log
            .iter()
            .map(|item| ToolResultRecord {
                header: RecordHeader {
                    id: format!("{}:tool:{}", turn_id, item.call_id),
                    session_id: session_id.to_string(),
                    turn_id: Some(turn_id.clone()),
                    timestamp: now,
                },
                call_id: item.call_id.clone(),
                tool: item.tool.clone(),
                payload: item.payload.clone(),
                is_error: item.is_error,
                truncated: item.truncated,
                size_bytes: item.size_bytes,
            })
            .collect(),
    };
    if let Err(err) = memory_store.append_turn(&talos_turn).await {
        warn!(
            session_id = %session_id,
            error = %err,
            "failed to append memory store turn"
        );
    }

    if let Err(err) = compact_workspace_context_if_needed(
        &session_id,
        &selected_model,
        llm_provider.clone(),
        memory_store.as_ref(),
        kernel_config,
    )
    .await
    {
        warn!(
            session_id = %session_id,
            error = %err,
            "failed to compact workspace memory context"
        );
    }

    Ok(GeneralTurnResult {
        output_text: final_text,
        output_items,
        stop_reason,
        usage,
        prompt_diagnostics,
        warnings,
    })
}
