use super::dream_memory_prompt::{
    date_for_timestamp, formation_user_prompt, DreamMemoryFormationMode,
    DREAM_MEMORY_FORMATION_PROMPT,
};
use super::memory_summary::{
    compact_summary_coverage, render_transcript, trim_transcript_units_to_budget,
    validate_memory_candidates,
};
use super::*;

const DREAM_MEMORY_TRANSCRIPT_MAX_CHARS: usize = 4_000;

/// Forms one bounded, contiguous Dream Memory batch for a session tail.
pub async fn form_dream_session_memory(
    params: MemoryFormationParams<'_>,
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
    let turn_limit = kernel_config
        .compaction
        .source_recent_messages_limit
        .max(kernel_config.compaction.source_recent_tool_exchanges_limit);
    let turns = session_artifact_store
        .load_turn_window(
            std::path::Path::new(&kernel_config.workspace_dir),
            session_id,
            talos_store::session::SessionTurnWindow::OldestAfter {
                seq: memory_processed_until_seq,
            },
            turn_limit,
        )
        .await?;
    let context_config = SessionPromptContextConfig {
        compact_cursor: (memory_processed_until_seq > 0).then_some(
            talos_prompt::CompactPromptCursor {
                covers_until_seq: memory_processed_until_seq,
            },
        ),
        recent_window: SessionPromptRecentWindow::OldestUncovered,
        recent_messages_limit: kernel_config.compaction.source_recent_messages_limit,
        recent_tool_results_limit: 0,
        tool_output_max_chars: kernel_config.compaction.tool_output_max_chars,
        recent_tool_exchanges_limit: kernel_config.compaction.source_recent_tool_exchanges_limit,
        tool_exchange_max_chars: kernel_config.compaction.tool_exchange_max_chars,
        include_summary: false,
        summary_recent_count: 0,
        summary_max_chars: 0,
    };
    let mut context = crate::session_context::build_recent_session_context(&turns, &context_config);
    let transcript_max_chars = kernel_config
        .compaction
        .recent_messages_max_chars
        .clamp(1, DREAM_MEMORY_TRANSCRIPT_MAX_CHARS);
    let message_max_chars = transcript_max_chars.saturating_sub(64).div_ceil(2).max(1);
    for message in &mut context.recent_messages {
        if message.text.chars().count() > message_max_chars {
            message.text = message.text.chars().take(message_max_chars).collect();
        }
    }
    let (units, _, _) =
        trim_transcript_units_to_budget(&context, transcript_max_chars.div_ceil(4), 0, 0);
    let transcript = render_transcript(&units);
    if transcript.trim().is_empty() {
        return Ok(MemoryFormationReport::default());
    }

    let turn_seq_by_id = turns
        .iter()
        .map(|turn| (turn.turn_id.clone(), turn.seq))
        .collect::<std::collections::HashMap<_, _>>();
    let occurred_at_by_seq = turns
        .iter()
        .map(|turn| (turn.seq, turn.timestamp.timestamp()))
        .collect::<std::collections::HashMap<_, _>>();
    let Some(coverage) =
        compact_summary_coverage(memory_processed_until_seq, &units, &turn_seq_by_id)
    else {
        return Ok(MemoryFormationReport::default());
    };
    let occurred_at = occurred_at_by_seq
        .get(&coverage.covers_until_seq)
        .copied()
        .ok_or_else(|| {
            talos_error::TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "missing persisted timestamp for Dream Memory session seq {}",
                    coverage.covers_until_seq
                ),
            )
        })?;
    let date = date_for_timestamp(occurred_at, &kernel_config.timezone)?;
    let snapshot = dream_memory_store.read_day(memory_scope, date).await?;
    let model = kernel_config
        .compaction
        .model
        .as_deref()
        .unwrap_or(selected_model);
    let response = llm_provider
        .chat(
            ChatRequest::builder()
                .model(model.to_string())
                .messages(vec![
                    ChatMessage::system(DREAM_MEMORY_FORMATION_PROMPT),
                    ChatMessage::user(formation_user_prompt(
                        DreamMemoryFormationMode::Maintenance,
                        &transcript,
                        &snapshot,
                    )?),
                ])
                .build(),
        )
        .await?;
    let output = serde_json::from_str::<DreamMemoryModelOutput>(response.content.trim()).map_err(
        |error| {
            talos_error::TalosError::memory_error(
                talos_error::TalosMemoryErrorKind::SummaryParseFailed,
                format!("parse Dream Memory json for session '{session_id}': {error}"),
            )
        },
    )?;
    let DreamMemoryModelOutput::Maintenance { memory_candidates } = output else {
        return Err(talos_error::TalosError::memory_error(
            talos_error::TalosMemoryErrorKind::SummaryParseFailed,
            format!("Dream Memory mode mismatch for session '{session_id}'"),
        ));
    };
    validate_memory_candidates(&memory_candidates)?;
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
                    candidates: memory_candidates,
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

    let mut report = MemoryFormationReport {
        covers_from_seq: Some(coverage.covers_from_seq),
        covers_until_seq: Some(coverage.covers_until_seq),
        ..MemoryFormationReport::default()
    };
    report.record_usage(&response.usage);
    Ok(report)
}
