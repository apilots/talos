use super::*;
use serde::{Deserialize, Serialize};

const MEMORY_PERSISTENCE_DECISION_PROMPT: &str = r#"Decide whether this sanitized session summary should be persisted.
Return ONLY valid JSON, no markdown fences, no explanation.

Schema:
{
  "decision": "skip" | "session_only" | "durable",
  "rationale": "short reason"
}

Decision rules:
- durable: stable user preferences, long-lived constraints, durable project facts, or ongoing goals worth keeping in durable memory
- session_only: useful session recap or pending work that helps near-term continuity but is not stable enough for durable memory
- skip: transient execution logs, flush noise, raw tool traces, unstable details, malformed or content-free summaries

Prefer skip over storing low-value or noisy information."#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MemoryPersistenceKind {
    Skip,
    SessionOnly,
    Durable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MemoryPersistenceDecision {
    decision: MemoryPersistenceKind,
    rationale: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SanitizedMemoryDecisionInput {
    pub(crate) user_intent: Option<String>,
    pub(crate) verified_outcomes: Vec<String>,
    pub(crate) stable_facts_discovered: Vec<String>,
    pub(crate) candidate_preferences_or_durable_rules: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryFlushPolicy {
    pub enforce_threshold: bool,
    pub persist_session_summary: bool,
    pub flush_workspace_memory: bool,
}

impl MemoryFlushPolicy {
    pub const fn new(
        enforce_threshold: bool,
        persist_session_summary: bool,
        flush_workspace_memory: bool,
    ) -> Self {
        Self {
            enforce_threshold,
            persist_session_summary,
            flush_workspace_memory,
        }
    }

    pub const fn for_compaction(flush_workspace_memory: bool) -> Self {
        Self::new(true, true, flush_workspace_memory)
    }

    pub const fn for_session_close(flush_workspace_memory: bool) -> Self {
        Self::new(false, false, flush_workspace_memory)
    }

    pub const fn has_persistence_target(self) -> bool {
        self.persist_session_summary || self.flush_workspace_memory
    }
}

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

fn render_workspace_summary_body(summary: &SessionMemorySummary) -> String {
    let body = render_summary_body(summary);
    if !body.is_empty() {
        return body;
    }

    let title = summary.title.trim();
    if title.is_empty() {
        String::new()
    } else {
        format!("**Facts:**\n- {}\n", title)
    }
}

fn sanitize_memory_line(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let forbidden_markers = [
        "session_id",
        "invocation_id",
        "message_id",
        "trace_id",
        "tool_call_id",
        "skip memory flush",
        "flush workspace memory",
        "prompt diagnostics",
        "status: execution started",
        "status: waiting for execution slot",
    ];
    if forbidden_markers
        .iter()
        .any(|marker| lower.contains(marker))
    {
        return None;
    }

    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return None;
    }

    Some(trimmed.to_string())
}

pub(crate) fn build_sanitized_memory_decision_input(
    context: &talos_memory::SessionPromptContext,
    summary: &SessionMemorySummary,
) -> SanitizedMemoryDecisionInput {
    let user_intent = context
        .recent_messages
        .iter()
        .rev()
        .find(|message| message.role == TalosMessageRole::User)
        .and_then(|message| sanitize_memory_line(&message.text));

    // Verified tool outcomes are not tracked separately in SessionMemorySummary yet.
    // Keep facts in the stable-facts section and only surface pending work here.
    let verified_outcomes = summary
        .open_tasks
        .iter()
        .filter_map(|item| sanitize_memory_line(item))
        .map(|item| format!("Pending task: {}", item))
        .collect::<Vec<_>>();

    let stable_facts_discovered = summary
        .facts
        .iter()
        .filter_map(|item| sanitize_memory_line(item))
        .collect::<Vec<_>>();

    let candidate_preferences_or_durable_rules = summary
        .preferences
        .iter()
        .filter_map(|item| sanitize_memory_line(item))
        .chain(
            summary
                .risks
                .iter()
                .filter_map(|item| sanitize_memory_line(item))
                .map(|item| format!("Constraint or risk: {}", item)),
        )
        .collect::<Vec<_>>();

    SanitizedMemoryDecisionInput {
        user_intent,
        verified_outcomes,
        stable_facts_discovered,
        candidate_preferences_or_durable_rules,
    }
}

fn render_sanitized_memory_decision_input(input: &SanitizedMemoryDecisionInput) -> Option<String> {
    let mut sections = Vec::new();
    if let Some(user_intent) = input.user_intent.as_ref() {
        sections.push(format!("User intent:\n- {}", user_intent));
    }
    if !input.verified_outcomes.is_empty() {
        sections.push(format!(
            "Verified outcomes:\n{}",
            input
                .verified_outcomes
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !input.stable_facts_discovered.is_empty() {
        sections.push(format!(
            "Stable facts discovered:\n{}",
            input
                .stable_facts_discovered
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !input.candidate_preferences_or_durable_rules.is_empty() {
        sections.push(format!(
            "Candidate preferences or durable rules:\n{}",
            input
                .candidate_preferences_or_durable_rules
                .iter()
                .map(|item| format!("- {}", item))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

async fn decide_memory_persistence(
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    sanitized_input: &str,
) -> TalosResult<MemoryPersistenceDecision> {
    let response = llm_provider
        .chat(
            ChatRequest::builder()
                .model(selected_model.to_string())
                .messages(vec![
                    ChatMessage::system(MEMORY_PERSISTENCE_DECISION_PROMPT),
                    ChatMessage::user(sanitized_input.to_string()),
                ])
                .build(),
        )
        .await?;

    serde_json::from_str(strip_json_fences(&response.content)).map_err(|err| {
        talos_error::TalosError::ToolError(format!(
            "parse memory persistence decision json: {}",
            err
        ))
    })
}

pub async fn flush_session_memory(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    policy: MemoryFlushPolicy,
) -> TalosResult<()> {
    if policy.has_persistence_target() {
        if policy.enforce_threshold {
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
            persist_session_summary = policy.persist_session_summary,
            flush_workspace_memory = policy.flush_workspace_memory,
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
        .model(selected_model.clone())
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
        tracing::debug!(session_id = %session_id, "skip memory flush because summary title is empty");
        return Ok(());
    }
    let workspace_body = render_workspace_summary_body(&summary);
    let session_summary_content = render_session_summary_content(&summary);
    if session_summary_content.is_empty() {
        return Ok(());
    }
    if !policy.persist_session_summary && policy.flush_workspace_memory {
        if !workspace_body.is_empty() {
            memory_store
                .append_workspace_memory(&talos_memory::WorkspaceMemoryRecord {
                    timestamp: Utc::now(),
                    session_id: session_id.to_string(),
                    title: summary.title.clone(),
                    content: workspace_body,
                })
                .await?;
        }
        return Ok(());
    }
    let sanitized_input = build_sanitized_memory_decision_input(&context, &summary);
    let Some(judge_input) = render_sanitized_memory_decision_input(&sanitized_input) else {
        tracing::debug!(session_id = %session_id, "skip memory flush because sanitized judge input is empty");
        return Ok(());
    };
    let persistence =
        decide_memory_persistence(&selected_model, llm_provider.clone(), &judge_input).await?;
    tracing::debug!(
        session_id = %session_id,
        decision = ?persistence.decision,
        rationale = %persistence.rationale,
        "memory persistence decision"
    );
    if persistence.decision == MemoryPersistenceKind::Skip {
        return Ok(());
    }
    let latest_seq = memory_store.latest_seq(session_id).await?;
    let source_turn_count = context
        .recent_messages
        .iter()
        .filter_map(|item| item.header.turn_id.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if policy.persist_session_summary {
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

    let should_append_workspace_memory = policy.flush_workspace_memory
        && !workspace_body.is_empty()
        && match persistence.decision {
            MemoryPersistenceKind::Skip => false,
            MemoryPersistenceKind::Durable => true,
            MemoryPersistenceKind::SessionOnly => !policy.persist_session_summary,
        };

    if should_append_workspace_memory {
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
        MemoryFlushPolicy::for_compaction(
            kernel_config.memory.workspace_memory_flush_enabled
                && kernel_config.memory.workspace_memory_flush_on_compaction,
        ),
    )
    .await
}
