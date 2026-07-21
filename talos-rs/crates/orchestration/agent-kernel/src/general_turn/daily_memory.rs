use super::memory_summary::{
    compact_summary_coverage, render_transcript, trim_transcript_units_to_budget,
    validate_memory_candidates,
};
use super::*;

const DAILY_MEMORY_TRANSCRIPT_MAX_CHARS: usize = 4_000;

const DAILY_MEMORY_FORMATION_PROMPT: &str = r#"Extract durable memory candidates from this conversation as JSON.
Return ONLY valid JSON, no markdown fences, no explanation.

Schema:
{
  "memory_candidates": [
    {
      "title": "durable memory title",
      "body": "durable fact, preference, decision, task, or episode",
      "kind": "preference" | "fact" | "decision" | "task" | "episode",
      "importance": "low" | "normal" | "high"
    }
  ]
}

Return at most 8 memory_candidates. Exclude transient logs and unstable details.
Use the same language as the conversation transcript for all field values."#;

#[derive(serde::Deserialize)]
struct DailyMemoryOutput {
    #[serde(default)]
    memory_candidates: Vec<talos_memory::MemoryCandidate>,
}

/// Forms one bounded, contiguous daily-memory batch for a session tail.
pub async fn form_daily_session_memory(
    params: MemoryFormationParams<'_>,
) -> TalosResult<MemoryFormationReport> {
    let MemoryFormationParams {
        session_id,
        selected_model,
        llm_provider,
        memory_writer,
        memory_scope,
        session_artifact_store,
        session_store,
        kernel_config,
    } = params;
    let memory_processed_until_seq = session_store
        .get_session_record(session_id)
        .await?
        .ok_or_else(|| talos_error::TalosError::ResourceNotFound(session_id.to_string()))?
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
        .clamp(1, DAILY_MEMORY_TRANSCRIPT_MAX_CHARS);
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
            talos_error::TalosError::ValidationError(format!(
                "missing persisted timestamp for daily memory session seq {}",
                coverage.covers_until_seq
            ))
        })?;
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
                    ChatMessage::system(DAILY_MEMORY_FORMATION_PROMPT),
                    ChatMessage::user(format!("Conversation transcript:\n{transcript}")),
                ])
                .build(),
        )
        .await?;
    let output =
        serde_json::from_str::<DailyMemoryOutput>(response.content.trim()).map_err(|error| {
            talos_error::TalosError::MemoryError {
                kind: talos_error::TalosMemoryErrorKind::SummaryParseFailed,
                message: format!("parse daily memory json for session '{session_id}': {error}"),
            }
        })?;
    validate_memory_candidates(&output.memory_candidates)?;
    memory_writer
        .write(
            memory_scope,
            MemoryWriteBatch {
                session_id: session_id.to_string(),
                occurred_at,
                source_from_seq: coverage.covers_from_seq,
                source_until_seq: coverage.covers_until_seq,
                candidates: output.memory_candidates,
            },
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
