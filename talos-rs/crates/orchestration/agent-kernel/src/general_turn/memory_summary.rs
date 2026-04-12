use super::*;

fn render_summary_body(summary: &SessionMemorySummary) -> String {
    let mut out = String::new();
    let sections = [
        ("**Facts:**", &summary.facts),
        ("**Preferences:**", &summary.preferences),
        ("**Open Tasks:**", &summary.open_tasks),
        ("**Risks:**", &summary.risks),
    ];
    for (heading, items) in sections {
        if items.is_empty() {
            continue;
        }
        out.push_str(heading);
        out.push('\n');
        for item in items {
            out.push_str("- ");
            out.push_str(item.trim());
            out.push('\n');
        }
        out.push('\n');
    }
    out.trim().to_string()
}

fn render_session_summary_content(summary: &SessionMemorySummary) -> String {
    let mut out = String::new();
    out.push_str("Title: ");
    out.push_str(summary.title.trim());
    out.push_str("\n\n");
    let body = render_summary_body(summary);
    out.push_str(body.trim());
    out.trim().to_string()
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
                tracing::debug!(session_id = %session_id, "skip memory flush because threshold is zero");
                return Ok(());
            }
            let session_bytes = memory_store.session_size_bytes(session_id).await?;
            if session_bytes < threshold {
                tracing::debug!(
                    session_id = %session_id,
                    session_bytes,
                    threshold,
                    "skip memory flush because session size is below threshold"
                );
                return Ok(());
            }
        }
    } else {
        tracing::debug!(
            session_id = %session_id,
            persist_session_summary,
            flush_workspace_memory,
            "skip memory flush because all persistence targets are disabled"
        );
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
        tracing::debug!(session_id = %session_id, "skip memory flush because session prompt context is empty");
        return Ok(());
    }
    let transcript = context
        .recent_messages
        .iter()
        .filter_map(|message| {
            let text = message.text.trim();
            if text.is_empty() {
                return None;
            }
            Some(match message.role {
                TalosMessageRole::User => format!("User: {}", text),
                TalosMessageRole::Assistant => format!("Assistant: {}", text),
                TalosMessageRole::System => format!("System: {}", text),
            })
        })
        .collect::<Vec<_>>()
        .join("\n");
    if transcript.trim().is_empty() {
        tracing::debug!(session_id = %session_id, "skip memory flush because transcript is empty");
        return Ok(());
    }

    let selected_model = selected_model.to_string();
    let llm_provider_for_summary = llm_provider.clone();
    let request = ChatRequest::builder()
        .model(selected_model)
        .messages(vec![
            ChatMessage::system(MEMORY_SUMMARY_PROMPT),
            ChatMessage::user(format!("Conversation transcript:\n{}", transcript)),
        ])
        .build();
    let response = llm_provider_for_summary.chat(request).await?;
    let summary: SessionMemorySummary = serde_json::from_str(strip_json_fences(&response.content))
        .map_err(|err| {
            talos_error::TalosError::ToolError(format!("parse memory summary json: {}", err))
        })?;
    if summary.title.trim().is_empty() {
        return Err(talos_error::TalosError::ToolError(format!(
            "memory summary title must not be empty for session {}",
            session_id
        )));
    }
    let workspace_body = render_summary_body(&summary);
    let session_summary_content = render_session_summary_content(&summary);
    if session_summary_content.is_empty() {
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
                content: session_summary_content,
            })
            .await?;
    }

    if flush_workspace_memory && !workspace_body.is_empty() {
        memory_store
            .append_workspace_memory(&talos_memory::WorkspaceMemoryRecord {
                timestamp: Utc::now(),
                session_id: session_id.to_string(),
                title: summary.title.clone(),
                content: workspace_body,
            })
            .await?;
    }

    Ok(())
}

pub(crate) async fn compact_workspace_context_if_needed(
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
