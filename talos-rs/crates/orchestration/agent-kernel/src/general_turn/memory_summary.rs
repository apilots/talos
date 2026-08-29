use super::dream_memory_prompt::{
    date_for_timestamp, formation_user_prompt, DreamMemoryFormationMode,
    DREAM_MEMORY_FORMATION_PROMPT,
};
use super::*;
use talos_error::TalosMemoryErrorKind;
use talos_llm_provider::Usage;

pub(super) fn validate_memory_candidates(
    candidates: &[talos_memory::MemoryCandidate],
) -> TalosResult<()> {
    if candidates.len() > 8
        || candidates.iter().any(|candidate| {
            candidate.title.trim().is_empty()
                || candidate.body.trim().is_empty()
                || candidate.title.chars().count() > 160
                || candidate.body.chars().count() > 1_000
        })
    {
        return Err(talos_error::TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "memory candidates are invalid".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryFormationReport {
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

impl MemoryFormationReport {
    pub(super) fn record_usage(&mut self, usage: &Usage) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryCompactionPolicy {
    pub compact_input_budget_tokens: Option<usize>,
}

impl MemoryCompactionPolicy {
    pub const fn for_compaction() -> Self {
        Self {
            compact_input_budget_tokens: None,
        }
    }

    pub const fn with_compaction_input_budget_tokens(mut self, budget: Option<usize>) -> Self {
        self.compact_input_budget_tokens = budget;
        self
    }
}

/// Stable inputs shared by compaction and daily long-term memory formation.
pub struct MemoryFormationParams<'a> {
    /// Session whose persisted transcript is processed.
    pub session_id: &'a str,
    /// Agent that produced new canonical Dream Memory content.
    pub producer_agent_id: &'a str,
    /// Session-selected model used when no compaction override exists.
    pub selected_model: &'a str,
    /// Provider used for the single structured extraction request.
    pub llm_provider: Arc<dyn LlmProvider>,
    /// Long-term memory write boundary.
    pub dream_memory_store: &'a dyn DreamMemoryFormationStore,
    /// User-scoped long-term memory destination.
    pub memory_scope: &'a MemoryScope,
    /// Persisted transcript and summary boundary.
    pub session_artifact_store: &'a dyn talos_store::session::SessionArtifactStore,
    /// Persisted memory cursor boundary.
    pub session_store: &'a dyn talos_store::session::SessionStore,
    /// Resolved per-user Kernel configuration.
    pub kernel_config: &'a AgentKernelConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompactableTranscriptUnitKind {
    HistoryTurn,
    ToolExchange,
    ToolResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CompactableTranscriptUnit {
    kind: CompactableTranscriptUnitKind,
    record_id: String,
    turn_id: Option<String>,
    timestamp: chrono::DateTime<chrono::Utc>,
    rendered: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CompactSummaryCoverage {
    pub(super) covers_from_seq: u64,
    pub(super) covers_until_seq: u64,
    pub(super) source_turn_count: usize,
}

fn render_summary_body(summary: &CompactionMemoryOutput) -> String {
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

fn render_session_summary_content(summary: &CompactionMemoryOutput) -> String {
    let mut out = String::new();
    out.push_str("Title: ");
    out.push_str(summary.title.trim());
    out.push_str("\n\n");
    let body = render_summary_body(summary);
    out.push_str(body.trim());
    out.trim().to_string()
}

fn render_transcript_line(message: &crate::TalosMessage) -> Option<String> {
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

fn render_tool_result_line(result: &crate::SessionToolResultContext) -> Option<String> {
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

fn render_tool_exchange_line(exchange: &crate::SessionToolExchangeContext) -> Option<String> {
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
        SessionToolExchangeOutcome::Succeeded => "ok",
        SessionToolExchangeOutcome::Failed => "error",
        SessionToolExchangeOutcome::Pending => "pending",
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

pub(super) fn render_transcript(units: &[CompactableTranscriptUnit]) -> String {
    units
        .iter()
        .map(|unit| unit.rendered.trim())
        .filter(|rendered| !rendered.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn build_compactable_transcript_units(
    context: &crate::SessionPromptContext,
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

pub(super) fn trim_transcript_units_to_budget(
    context: &crate::SessionPromptContext,
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

pub(super) fn compact_summary_coverage(
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

async fn request_compaction_output(
    session_id: &str,
    selected_model: &str,
    llm_provider: Arc<dyn LlmProvider>,
    transcript: &str,
    snapshot: &talos_memory::DreamMemoryDaySnapshot,
) -> TalosResult<(CompactionMemoryOutput, Usage)> {
    let response = llm_provider
        .chat(
            ChatRequest::builder()
                .model(selected_model.to_string())
                .messages(vec![
                    ChatMessage::system(DREAM_MEMORY_FORMATION_PROMPT),
                    ChatMessage::user(formation_user_prompt(
                        DreamMemoryFormationMode::Compaction,
                        transcript,
                        snapshot,
                    )?),
                ])
                .build(),
        )
        .await?;
    let parsed = serde_json::from_str::<DreamMemoryModelOutput>(response.content.trim()).map_err(
        |error| {
            talos_error::TalosError::memory_error(
                TalosMemoryErrorKind::SummaryParseFailed,
                format!("parse compaction json for session '{session_id}': {error}"),
            )
        },
    )?;
    let DreamMemoryModelOutput::Compaction {
        title,
        facts,
        preferences,
        open_tasks,
        risks,
        memory_candidates,
    } = parsed
    else {
        return Err(talos_error::TalosError::memory_error(
            TalosMemoryErrorKind::SummaryParseFailed,
            format!("Dream Memory mode mismatch for session '{session_id}'"),
        ));
    };
    let parsed = CompactionMemoryOutput {
        title,
        facts,
        preferences,
        open_tasks,
        risks,
        memory_candidates,
    };
    if parsed.title.trim().is_empty() {
        return Err(talos_error::TalosError::memory_error(
            TalosMemoryErrorKind::SummaryParseFailed,
            format!("compaction title is blank for session '{session_id}'"),
        ));
    }
    validate_memory_candidates(&parsed.memory_candidates).map_err(|error| {
        talos_error::TalosError::memory_error(
            TalosMemoryErrorKind::SummaryParseFailed,
            format!("validate compaction for session '{session_id}': {error}"),
        )
    })?;
    Ok((parsed, response.usage))
}

async fn latest_summary_compact_cursor(
    session_artifact_store: &dyn talos_store::session::SessionArtifactStore,
    workspace_root: &std::path::Path,
    session_id: &str,
) -> TalosResult<Option<talos_prompt::CompactPromptCursor>> {
    Ok(session_artifact_store
        .load_summaries(workspace_root, session_id, 1)
        .await?
        .into_iter()
        .last()
        .map(|summary| talos_prompt::CompactPromptCursor {
            covers_until_seq: summary.covers_until_seq,
        }))
}

pub async fn compact_session_memory(
    params: MemoryFormationParams<'_>,
    policy: MemoryCompactionPolicy,
) -> TalosResult<MemoryFormationReport> {
    let MemoryFormationParams {
        session_id,
        producer_agent_id,
        selected_model,
        llm_provider,
        dream_memory_store,
        memory_scope,
        session_artifact_store,
        session_store,
        kernel_config,
    } = params;
    let mut usage_report = MemoryFormationReport::default();
    let compact_cursor = latest_summary_compact_cursor(
        session_artifact_store,
        std::path::Path::new(&kernel_config.workspace_dir),
        session_id,
    )
    .await?;
    let previous_covers_until_seq = compact_cursor
        .map(|cursor| cursor.covers_until_seq)
        .unwrap_or_default();
    let memory_processed_until_seq = session_store
        .get_session_record(session_id)
        .await?
        .ok_or_else(|| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                session_id.to_string(),
            )
        })?
        .memory_processed_until_seq;
    let context = crate::build_session_prompt_context(
        session_artifact_store,
        std::path::Path::new(&kernel_config.workspace_dir),
        session_id,
        &SessionPromptContextConfig {
            compact_cursor,
            recent_window: SessionPromptRecentWindow::OldestUncovered,
            recent_messages_limit: kernel_config.compaction.source_recent_messages_limit,
            recent_tool_results_limit: kernel_config.compaction.source_recent_tool_results_limit,
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
        tracing::debug!(session_id = %session_id, "skip compaction because session prompt context is empty");
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
        tracing::debug!(session_id = %session_id, "skip compaction because transcript is empty");
        return Ok(usage_report);
    }

    let turns = session_artifact_store
        .load_turn_window(
            std::path::Path::new(&kernel_config.workspace_dir),
            session_id,
            talos_store::session::SessionTurnWindow::OldestAfter {
                seq: previous_covers_until_seq,
            },
            kernel_config
                .compaction
                .source_recent_messages_limit
                .max(kernel_config.compaction.source_recent_tool_results_limit)
                .max(kernel_config.compaction.source_recent_tool_exchanges_limit),
        )
        .await?;
    let turn_seq_by_id = turns
        .iter()
        .map(|turn| (turn.turn_id.clone(), turn.seq))
        .collect::<std::collections::HashMap<_, _>>();
    let occurred_at_by_seq = turns
        .into_iter()
        .map(|turn| (turn.seq, turn.timestamp.timestamp()))
        .collect::<std::collections::HashMap<_, _>>();
    let Some(coverage) =
        compact_summary_coverage(previous_covers_until_seq, &compacted_units, &turn_seq_by_id)
    else {
        tracing::debug!(
            session_id = %session_id,
            previous_covers_until_seq,
            "skip session summary persistence because compact input has no contiguous uncovered history"
        );
        return Ok(usage_report);
    };
    let occurred_at = occurred_at_by_seq
        .get(&coverage.covers_until_seq)
        .copied()
        .ok_or_else(|| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "missing persisted timestamp for compacted session seq {}",
                    coverage.covers_until_seq
                ),
            )
        })?;
    let date = date_for_timestamp(occurred_at, &kernel_config.timezone)?;
    let snapshot = dream_memory_store.read_day(memory_scope, date).await?;
    let selected_model = kernel_config
        .compaction
        .model
        .clone()
        .unwrap_or_else(|| selected_model.to_string());
    let (summary, summary_usage) = request_compaction_output(
        session_id,
        &selected_model,
        llm_provider,
        &transcript,
        &snapshot,
    )
    .await?;
    usage_report.record_usage(&summary_usage);
    let session_summary_content = render_session_summary_content(&summary);
    if memory_processed_until_seq == previous_covers_until_seq {
        dream_memory_store
            .write(
                memory_scope,
                talos_memory::DreamMemoryWriteRequest::from_snapshot(
                    &snapshot,
                    talos_memory::MemoryAgentId::parse(producer_agent_id.to_string()).map_err(
                        |error| TalosError::internal(format!("invalid producer Agent id: {error}")),
                    )?,
                    MemoryWriteBatch {
                        session_id: session_id.to_string(),
                        occurred_at,
                        source_from_seq: coverage.covers_from_seq,
                        source_until_seq: coverage.covers_until_seq,
                        candidates: summary.memory_candidates,
                    },
                ),
            )
            .await?;
        session_store
            .advance_memory_processed_until_seq(
                session_id,
                memory_processed_until_seq,
                coverage.covers_until_seq,
            )
            .await?;
    }

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
    session_artifact_store
        .save_summary(
            std::path::Path::new(&kernel_config.workspace_dir),
            &SummaryRecord {
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
            },
        )
        .await?;

    Ok(usage_report)
}

fn sum_optional_tokens(left: Option<u32>, right: Option<u32>) -> Option<u32> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
