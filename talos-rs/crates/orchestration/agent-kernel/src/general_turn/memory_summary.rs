use super::*;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use talos_error::TalosMemoryErrorKind;
use talos_llm_provider::Usage;

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

const MEMORY_JSON_RETRY_LIMIT: usize = 3;

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

trait MemoryJsonValidator {
    fn validate(&self) -> TalosResult<()>;
}

impl MemoryJsonValidator for SessionMemorySummary {
    fn validate(&self) -> TalosResult<()> {
        if self.title.trim().is_empty() {
            return Err(talos_error::TalosError::ToolError(
                "memory summary title must not be blank".to_string(),
            ));
        }
        Ok(())
    }
}

impl MemoryJsonValidator for MemoryPersistenceDecision {
    fn validate(&self) -> TalosResult<()> {
        if self.rationale.trim().is_empty() {
            return Err(talos_error::TalosError::ToolError(
                "memory persistence rationale must not be blank".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFlushUsageReport {
    pub request_count: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub covers_from_seq: Option<u64>,
    pub covers_until_seq: Option<u64>,
    pub trimmed_context_unit_count: u32,
    pub trimmed_context_unit_kind: Option<CompactionTrimUnitKind>,
    pub cache_creation_input_tokens: Option<u32>,
    pub cache_read_input_tokens: Option<u32>,
}

impl MemoryFlushUsageReport {
    fn record_usage(&mut self, usage: &Usage) {
        self.request_count = self.request_count.saturating_add(1);
        self.input_tokens = self.input_tokens.saturating_add(usage.prompt_tokens);
        self.output_tokens = self.output_tokens.saturating_add(usage.completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(usage.total_tokens);
        self.cache_creation_input_tokens = sum_optional_tokens(
            self.cache_creation_input_tokens,
            usage.cache_creation_input_tokens,
        );
        self.cache_read_input_tokens =
            sum_optional_tokens(self.cache_read_input_tokens, usage.cache_read_input_tokens);
    }
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
    pub compact_input_budget_tokens: Option<usize>,
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
            compact_input_budget_tokens: None,
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

    pub const fn with_compaction_input_budget_tokens(mut self, budget: Option<usize>) -> Self {
        self.compact_input_budget_tokens = budget;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompactableTranscriptUnitKind {
    HistoryTurn,
    ToolExchange,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactableTranscriptUnit {
    kind: CompactableTranscriptUnitKind,
    record_id: String,
    turn_id: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>,
    rendered: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactSummaryCoverage {
    covers_from_seq: u64,
    covers_until_seq: u64,
    source_turn_count: usize,
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

fn render_transcript_line(message: &talos_memory::TalosMessage) -> Option<String> {
    let text = message.text.trim();
    if text.is_empty() {
        return None;
    }
    Some(match message.role {
        TalosMessageRole::User => format!("User: {}", text),
        TalosMessageRole::Assistant => format!("Assistant: {}", text),
        TalosMessageRole::System => format!("System: {}", text),
    })
}

fn render_tool_result_line(result: &talos_memory::SessionToolResultContext) -> Option<String> {
    let content = result.content.trim();
    if content.is_empty() {
        return None;
    }

    let status = if result.is_error { "error" } else { "ok" };
    let suffix = if result.truncated { " [truncated]" } else { "" };
    Some(format!(
        "Tool {} result ({status}){suffix}: {}",
        result.tool_name, content
    ))
}

fn render_tool_exchange_line(
    exchange: &talos_memory::SessionToolExchangeContext,
) -> Option<String> {
    let arguments = exchange.arguments_preview.trim();
    let output_preview = exchange
        .output_preview
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    if arguments.is_empty() && output_preview.is_none() {
        return None;
    }

    let outcome = match exchange.outcome {
        talos_memory::SessionToolExchangeOutcome::Succeeded => "ok",
        talos_memory::SessionToolExchangeOutcome::Failed => "error",
        talos_memory::SessionToolExchangeOutcome::Pending => "pending",
    };
    let mut rendered = format!(
        "Tool exchange {} ({outcome}) args: {}",
        exchange.tool_name, arguments
    );
    if let Some(output_preview) = output_preview {
        let suffix = if exchange.output_truncated {
            " [truncated]"
        } else {
            ""
        };
        rendered.push_str(&format!(" | preview{suffix}: {output_preview}"));
    }
    Some(rendered)
}

fn estimate_transcript_tokens(text: &str) -> usize {
    let chars = text.chars().count();
    if chars == 0 {
        0
    } else {
        chars.div_ceil(4)
    }
}

fn render_transcript(units: &[CompactableTranscriptUnit]) -> String {
    units
        .iter()
        .map(|unit| unit.rendered.trim())
        .filter(|rendered| !rendered.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_compactable_transcript_units(
    context: &talos_memory::SessionPromptContext,
) -> Vec<CompactableTranscriptUnit> {
    let mut units = Vec::new();
    for message in &context.recent_messages {
        let turn_id = message.header.turn_id.clone();
        let rendered = match render_transcript_line(message) {
            Some(rendered) => rendered,
            None => continue,
        };
        let can_append = turn_id.is_some()
            && units
                .last()
                .map(|unit: &CompactableTranscriptUnit| {
                    unit.kind == CompactableTranscriptUnitKind::HistoryTurn
                        && unit.turn_id == turn_id
                })
                .unwrap_or(false);
        if can_append {
            let unit = units.last_mut().expect("last unit should exist");
            unit.rendered.push('\n');
            unit.rendered.push_str(&rendered);
        } else {
            units.push(CompactableTranscriptUnit {
                kind: CompactableTranscriptUnitKind::HistoryTurn,
                record_id: message.header.id.clone(),
                turn_id,
                timestamp: message.header.timestamp,
                rendered,
            });
        }
    }

    for result in &context.recent_tool_results {
        let Some(rendered) = render_tool_result_line(result) else {
            continue;
        };
        units.push(CompactableTranscriptUnit {
            kind: CompactableTranscriptUnitKind::ToolResult,
            record_id: result.header.id.clone(),
            turn_id: result.header.turn_id.clone(),
            timestamp: result.header.timestamp,
            rendered,
        });
    }

    for exchange in &context.recent_tool_exchanges {
        let Some(rendered) = render_tool_exchange_line(exchange) else {
            continue;
        };
        units.push(CompactableTranscriptUnit {
            kind: CompactableTranscriptUnitKind::ToolExchange,
            record_id: exchange.header.id.clone(),
            turn_id: exchange.header.turn_id.clone(),
            timestamp: exchange.header.timestamp,
            rendered,
        });
    }

    units.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| match (&left.kind, &right.kind) {
                (
                    CompactableTranscriptUnitKind::HistoryTurn,
                    CompactableTranscriptUnitKind::ToolExchange
                    | CompactableTranscriptUnitKind::ToolResult,
                ) => std::cmp::Ordering::Less,
                (
                    CompactableTranscriptUnitKind::ToolExchange,
                    CompactableTranscriptUnitKind::ToolResult,
                ) => std::cmp::Ordering::Less,
                (
                    CompactableTranscriptUnitKind::ToolResult,
                    CompactableTranscriptUnitKind::HistoryTurn
                    | CompactableTranscriptUnitKind::ToolExchange,
                )
                | (
                    CompactableTranscriptUnitKind::ToolExchange,
                    CompactableTranscriptUnitKind::HistoryTurn,
                ) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    });
    units
}

fn trim_transcript_units_to_budget(
    context: &talos_memory::SessionPromptContext,
    transcript_budget_tokens: usize,
    tool_output_keep_last: usize,
    tool_exchange_keep_last: usize,
) -> (
    Vec<CompactableTranscriptUnit>,
    usize,
    Option<CompactionTrimUnitKind>,
) {
    let mut units = build_compactable_transcript_units(context);
    if transcript_budget_tokens == 0 {
        return (units, 0, None);
    }

    let mut trimmed_units = 0;
    let mut trimmed_kind = None;
    loop {
        let transcript = render_transcript(&units);
        if estimate_transcript_tokens(&transcript) <= transcript_budget_tokens {
            return (units, trimmed_units, trimmed_kind);
        }

        let tool_result_indexes = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| {
                (unit.kind == CompactableTranscriptUnitKind::ToolResult).then_some(index)
            })
            .collect::<Vec<_>>();
        if tool_result_indexes.len() > tool_output_keep_last {
            let index = tool_result_indexes[0];
            units.remove(index);
            trimmed_units += 1;
            trimmed_kind.get_or_insert(CompactionTrimUnitKind::OldToolOutput);
            continue;
        }

        let tool_exchange_indexes = units
            .iter()
            .enumerate()
            .filter_map(|(index, unit)| {
                (unit.kind == CompactableTranscriptUnitKind::ToolExchange).then_some(index)
            })
            .collect::<Vec<_>>();
        if tool_exchange_indexes.len() > tool_exchange_keep_last {
            let index = tool_exchange_indexes[0];
            units.remove(index);
            trimmed_units += 1;
            trimmed_kind.get_or_insert(CompactionTrimUnitKind::OldAssistantToolExchange);
            continue;
        }

        if let Some(index) = units
            .iter()
            .rposition(|unit| unit.kind == CompactableTranscriptUnitKind::HistoryTurn)
        {
            units.remove(index);
            trimmed_units += 1;
            if trimmed_kind.is_none() {
                trimmed_kind = Some(CompactionTrimUnitKind::HistoryTurn);
            }
            continue;
        }

        return (units, trimmed_units, trimmed_kind);
    }
}

fn compact_summary_coverage(
    previous_covers_until_seq: u64,
    units: &[CompactableTranscriptUnit],
    turn_seq_by_id: &std::collections::HashMap<String, u64>,
) -> Option<CompactSummaryCoverage> {
    let source_seqs = units
        .iter()
        .filter(|unit| unit.kind == CompactableTranscriptUnitKind::HistoryTurn)
        .filter_map(|unit| unit.turn_id.as_ref())
        .filter_map(|turn_id| turn_seq_by_id.get(turn_id).copied())
        .filter(|seq| *seq > previous_covers_until_seq)
        .collect::<std::collections::BTreeSet<_>>();
    let mut covers_until_seq = previous_covers_until_seq;
    let mut source_turn_count = 0;

    loop {
        let next_seq = covers_until_seq.saturating_add(1);
        if !source_seqs.contains(&next_seq) {
            break;
        }
        covers_until_seq = next_seq;
        source_turn_count += 1;
    }

    (source_turn_count > 0).then_some(CompactSummaryCoverage {
        covers_from_seq: previous_covers_until_seq.saturating_add(1),
        covers_until_seq,
        source_turn_count,
    })
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

async fn request_json_with_retries<T>(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    system_prompt: &str,
    user_prompt: String,
    label: &str,
    parse_error_kind: TalosMemoryErrorKind,
) -> TalosResult<(T, Usage)>
where
    T: DeserializeOwned + MemoryJsonValidator,
{
    let mut accumulated_usage = Usage::default();
    let retry_suffix = "\n\nPrevious response was not valid JSON for the required schema. Retry and return ONLY valid JSON with all required fields. Do not add explanation, markdown, or extra text.";
    let mut current_user_prompt = user_prompt.clone();
    let mut last_error = None;

    for attempt in 1..=MEMORY_JSON_RETRY_LIMIT {
        let response = llm_provider
            .chat(
                ChatRequest::builder()
                    .model(selected_model.to_string())
                    .messages(vec![
                        ChatMessage::system(system_prompt),
                        ChatMessage::user(current_user_prompt.clone()),
                    ])
                    .build(),
            )
            .await?;
        accumulated_usage.prompt_tokens = accumulated_usage
            .prompt_tokens
            .saturating_add(response.usage.prompt_tokens);
        accumulated_usage.completion_tokens = accumulated_usage
            .completion_tokens
            .saturating_add(response.usage.completion_tokens);
        accumulated_usage.total_tokens = accumulated_usage
            .total_tokens
            .saturating_add(response.usage.total_tokens);
        accumulated_usage.cache_creation_input_tokens = sum_optional_tokens(
            accumulated_usage.cache_creation_input_tokens,
            response.usage.cache_creation_input_tokens,
        );
        accumulated_usage.cache_read_input_tokens = sum_optional_tokens(
            accumulated_usage.cache_read_input_tokens,
            response.usage.cache_read_input_tokens,
        );

        match serde_json::from_str::<T>(strip_json_fences(&response.content)) {
            Ok(parsed) => match parsed.validate() {
                Ok(()) => return Ok((parsed, accumulated_usage)),
                Err(err) => {
                    tracing::warn!(
                        session_id = %session_id,
                        attempt,
                        error = %err,
                        response_chars = response.content.chars().count(),
                        label,
                        "received invalid structured content from memory helper model response"
                    );
                    last_error = Some(err.to_string());
                    if attempt == MEMORY_JSON_RETRY_LIMIT {
                        break;
                    }
                    current_user_prompt = format!("{user_prompt}{retry_suffix}");
                }
            },
            Err(err) => {
                tracing::warn!(
                    session_id = %session_id,
                    attempt,
                    error = %err,
                    response_chars = response.content.chars().count(),
                    label,
                    "received invalid json from memory helper model response"
                );
                last_error = Some(format!("{err}"));
                if attempt == MEMORY_JSON_RETRY_LIMIT {
                    break;
                }
                current_user_prompt = format!("{user_prompt}{retry_suffix}");
            }
        }
    }

    Err(talos_error::TalosError::MemoryError {
        kind: parse_error_kind,
        message: format!(
            "parse {label} json after {MEMORY_JSON_RETRY_LIMIT} attempts: {}",
            last_error.unwrap_or_else(|| "unknown parse error".to_string())
        ),
    })
}

async fn decide_memory_persistence(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    sanitized_input: &str,
) -> TalosResult<(MemoryPersistenceDecision, Usage)> {
    request_json_with_retries::<MemoryPersistenceDecision>(
        session_id,
        selected_model,
        llm_provider,
        MEMORY_PERSISTENCE_DECISION_PROMPT,
        sanitized_input.to_string(),
        "memory persistence decision",
        TalosMemoryErrorKind::PersistenceDecisionParseFailed,
    )
    .await
}

async fn latest_summary_compact_cursor(
    memory_store: &MemoryStore,
    session_id: &str,
    enabled: bool,
) -> TalosResult<Option<talos_memory::CompactPromptCursor>> {
    if !enabled {
        return Ok(None);
    }

    Ok(memory_store
        .load_summaries(session_id, 1)
        .await?
        .into_iter()
        .last()
        .map(|summary| talos_memory::CompactPromptCursor {
            covers_until_seq: summary.covers_until_seq,
        }))
}

pub async fn flush_session_memory_with_report(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    policy: MemoryFlushPolicy,
) -> TalosResult<MemoryFlushUsageReport> {
    let mut usage_report = MemoryFlushUsageReport::default();
    if policy.has_persistence_target() {
        if policy.enforce_threshold {
            let threshold = kernel_config.memory.compaction_threshold_bytes as u64;
            if threshold == 0 {
                tracing::debug!(session_id = %session_id, "skip memory flush because threshold is zero");
                return Ok(usage_report);
            }
            let session_bytes = memory_store.session_size_bytes(session_id).await?;
            if session_bytes < threshold {
                tracing::debug!(
                    session_id = %session_id,
                    session_bytes,
                    threshold,
                    "skip memory flush because session size is below threshold"
                );
                return Ok(usage_report);
            }
        }
    } else {
        tracing::debug!(
            session_id = %session_id,
            persist_session_summary = policy.persist_session_summary,
            flush_workspace_memory = policy.flush_workspace_memory,
            "skip memory flush because all persistence targets are disabled"
        );
        return Ok(usage_report);
    }

    let compact_cursor =
        latest_summary_compact_cursor(memory_store, session_id, policy.persist_session_summary)
            .await?;
    let previous_covers_until_seq = compact_cursor
        .map(|cursor| cursor.covers_until_seq)
        .unwrap_or_default();
    let context = memory_store
        .build_session_prompt_context(
            session_id,
            &SessionPromptContextConfig {
                compact_cursor,
                recent_window: SessionPromptRecentWindow::OldestUncovered,
                recent_messages_limit: kernel_config.compaction.source_recent_messages_limit,
                recent_tool_results_limit: kernel_config
                    .compaction
                    .source_recent_tool_results_limit,
                tool_output_max_chars: kernel_config.compaction.tool_output_max_chars,
                recent_tool_exchanges_limit: kernel_config
                    .compaction
                    .source_recent_tool_exchanges_limit,
                tool_exchange_max_chars: kernel_config.compaction.tool_exchange_max_chars,
                include_summary: false,
                summary_recent_count: 0,
                summary_max_chars: 0,
            },
        )
        .await?;
    if context.recent_messages.is_empty()
        && context.recent_tool_results.is_empty()
        && context.recent_tool_exchanges.is_empty()
    {
        tracing::debug!(session_id = %session_id, "skip memory flush because session prompt context is empty");
        return Ok(usage_report);
    }
    let mut compaction_context = context.clone();
    if let Some(transcript_budget_tokens) = policy.compact_input_budget_tokens {
        let (trimmed_units, trimmed_unit_count, trimmed_unit_kind) =
            trim_transcript_units_to_budget(
                &compaction_context,
                transcript_budget_tokens,
                kernel_config.compaction.tool_output_keep_last,
                kernel_config.compaction.tool_exchange_keep_last,
            );
        if trimmed_unit_count > 0 {
            tracing::debug!(
                session_id = %session_id,
                transcript_budget_tokens,
                trimmed_unit_count,
                trimmed_unit_kind = ?trimmed_unit_kind,
                "trimmed transcript units before memory compaction"
            );
        }
        usage_report.trimmed_context_unit_count = trimmed_unit_count as u32;
        usage_report.trimmed_context_unit_kind = trimmed_unit_kind;
        let kept_history_turn_ids = trimmed_units
            .iter()
            .filter_map(|unit| {
                (unit.kind == CompactableTranscriptUnitKind::HistoryTurn)
                    .then(|| unit.turn_id.clone())
                    .flatten()
            })
            .collect::<std::collections::HashSet<_>>();
        let kept_tool_result_ids = trimmed_units
            .iter()
            .filter_map(|unit| {
                (unit.kind == CompactableTranscriptUnitKind::ToolResult)
                    .then_some(unit.record_id.clone())
            })
            .collect::<std::collections::HashSet<_>>();
        let kept_tool_exchange_ids = trimmed_units
            .iter()
            .filter_map(|unit| {
                (unit.kind == CompactableTranscriptUnitKind::ToolExchange)
                    .then_some(unit.record_id.clone())
            })
            .collect::<std::collections::HashSet<_>>();
        compaction_context.recent_messages.retain(|message| {
            message
                .header
                .turn_id
                .as_ref()
                .is_some_and(|turn_id| kept_history_turn_ids.contains(turn_id))
        });
        compaction_context
            .recent_tool_results
            .retain(|result| kept_tool_result_ids.contains(&result.header.id));
        compaction_context
            .recent_tool_exchanges
            .retain(|exchange| kept_tool_exchange_ids.contains(&exchange.header.id));
    }
    let compacted_units = build_compactable_transcript_units(&compaction_context);
    let transcript = render_transcript(&compacted_units);
    if transcript.trim().is_empty() {
        tracing::debug!(session_id = %session_id, "skip memory flush because transcript is empty");
        return Ok(usage_report);
    }

    let selected_model = kernel_config
        .compaction
        .model
        .clone()
        .unwrap_or_else(|| selected_model.to_string());
    let llm_provider_for_summary = llm_provider.clone();
    let transcript_prompt = format!("Conversation transcript:\n{}", transcript);
    let (summary, summary_usage) = request_json_with_retries::<SessionMemorySummary>(
        session_id,
        &selected_model,
        llm_provider_for_summary,
        MEMORY_SUMMARY_PROMPT,
        transcript_prompt,
        "memory summary",
        TalosMemoryErrorKind::SummaryParseFailed,
    )
    .await?;
    usage_report.record_usage(&summary_usage);
    if summary.title.trim().is_empty() {
        tracing::debug!(session_id = %session_id, "skip memory flush because summary title is empty");
        return Ok(usage_report);
    }
    let workspace_body = render_workspace_summary_body(&summary);
    let session_summary_content = render_session_summary_content(&summary);
    if session_summary_content.is_empty() {
        return Ok(usage_report);
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
        return Ok(usage_report);
    }
    let sanitized_input = build_sanitized_memory_decision_input(&compaction_context, &summary);
    let Some(judge_input) = render_sanitized_memory_decision_input(&sanitized_input) else {
        tracing::debug!(session_id = %session_id, "skip memory flush because sanitized judge input is empty");
        return Ok(usage_report);
    };
    let (persistence, persistence_usage) = decide_memory_persistence(
        session_id,
        &selected_model,
        llm_provider.clone(),
        &judge_input,
    )
    .await?;
    usage_report.record_usage(&persistence_usage);
    tracing::debug!(
        session_id = %session_id,
        decision = ?persistence.decision,
        rationale = %persistence.rationale,
        "memory persistence decision"
    );
    if persistence.decision == MemoryPersistenceKind::Skip {
        return Ok(usage_report);
    }
    let turn_seq_by_id = memory_store
        .load_session_turns(session_id)
        .await?
        .into_iter()
        .map(|turn| (turn.turn_id, turn.seq))
        .collect::<std::collections::HashMap<_, _>>();
    let coverage =
        compact_summary_coverage(previous_covers_until_seq, &compacted_units, &turn_seq_by_id);
    if policy.persist_session_summary {
        if let Some(coverage) = coverage {
            tracing::debug!(
                session_id = %session_id,
                previous_covers_until_seq,
                covers_from_seq = coverage.covers_from_seq,
                covers_until_seq = coverage.covers_until_seq,
                source_turn_count = coverage.source_turn_count,
                compacted_unit_count = compacted_units.len(),
                "compact summary coverage advanced"
            );
            usage_report.covers_from_seq = Some(coverage.covers_from_seq);
            usage_report.covers_until_seq = Some(coverage.covers_until_seq);
            memory_store
                .save_summary(&SummaryRecord {
                    version: 2,
                    header: RecordHeader {
                        id: Uuid::new_v4().to_string(),
                        session_id: session_id.to_string(),
                        turn_id: None,
                        timestamp: Utc::now(),
                    },
                    covers_until_seq: coverage.covers_until_seq,
                    source_turn_count: coverage.source_turn_count,
                    content: session_summary_content,
                })
                .await?;
        } else {
            tracing::debug!(
                session_id = %session_id,
                previous_covers_until_seq,
                "skip session summary persistence because compact input has no contiguous uncovered history"
            );
        }
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

    Ok(usage_report)
}

pub async fn flush_session_memory(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    memory_store: &MemoryStore,
    kernel_config: &AgentKernelConfig,
    policy: MemoryFlushPolicy,
) -> TalosResult<()> {
    let _ = flush_session_memory_with_report(
        session_id,
        selected_model,
        llm_provider,
        memory_store,
        kernel_config,
        policy,
    )
    .await?;
    Ok(())
}

fn sum_optional_tokens(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
