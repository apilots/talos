use super::*;
use talos_prompt::PromptSectionKind;
use talos_types::talos::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger,
};

pub(super) fn count_section_kind(
    sections: &[talos_prompt::PromptSection],
    kind: PromptSectionKind,
) -> usize {
    sections
        .iter()
        .filter(|section| section.kind == kind)
        .count()
}

fn history_sections_were_dropped(prompt_assembly: &talos_prompt::PromptAssembly) -> bool {
    prompt_assembly
        .diagnostics
        .dropped_sections
        .iter()
        .any(|kind| {
            matches!(
                kind,
                PromptSectionKind::RecentMessages | PromptSectionKind::SessionSummary
            )
        })
}

fn prompt_budget_usage_percent(window_usage: &talos_prompt::PromptWindowUsage) -> Option<u8> {
    if !window_usage.window_resolved {
        return None;
    }
    if window_usage.effective_prompt_budget == 0 {
        return Some(100);
    }
    let usage_percent =
        (window_usage.total_tokens.saturating_mul(100)) / window_usage.effective_prompt_budget;
    Some(usage_percent.min(100) as u8)
}

pub(super) fn resolve_compaction_input_budget_tokens(
    model_info: &talos_types::ModelInfo,
) -> Option<usize> {
    let context_window = model_info.model.context_window as usize;
    if context_window == 0 {
        return None;
    }

    Some((context_window / 2).clamp(256, 16_384).min(context_window))
}

pub(super) fn determine_history_compaction_trigger(
    prompt_assembly: &talos_prompt::PromptAssembly,
    kernel_config: &AgentKernelConfig,
) -> Option<SessionHistoryCompactionTrigger> {
    let history_reduction_needed = history_sections_were_dropped(prompt_assembly);
    let window_usage = &prompt_assembly.diagnostics.window_usage;
    if !window_usage.window_resolved {
        return None;
    }

    if !window_usage.within_effective_budget {
        Some(SessionHistoryCompactionTrigger::BudgetExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.hard_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::HardThresholdExceeded)
    } else if prompt_budget_usage_percent(window_usage)
        .is_some_and(|usage| usage >= kernel_config.compaction.trigger_ratio_percent)
    {
        Some(SessionHistoryCompactionTrigger::ThresholdExceeded)
    } else if history_reduction_needed {
        Some(SessionHistoryCompactionTrigger::HistorySectionsDropped)
    } else {
        None
    }
}

pub(super) fn is_recoverable_compaction_parse_error(error: &talos_error::TalosError) -> bool {
    error.memory_error_kind() == Some(talos_error::TalosMemoryErrorKind::SummaryParseFailed)
}

pub(super) async fn emit_context_compaction_stream_event(
    stream_tx: Option<&mpsc::Sender<AgentKernelEvent>>,
    trigger: ContextCompactionTrigger,
    stage: ContextCompactionStage,
    outcome: Option<ContextCompactionOutcome>,
    metrics: Option<ContextCompactionMetrics>,
    message: Option<String>,
) {
    emit_stream_event(
        stream_tx,
        AgentKernelEventPayload::ContextCompaction(crate::ContextCompactionStreamData {
            trigger,
            stage,
            outcome,
            metrics,
            message,
        }),
    )
    .await;
}

pub(super) fn map_context_compaction_trigger(
    trigger: SessionHistoryCompactionTrigger,
) -> ContextCompactionTrigger {
    match trigger {
        SessionHistoryCompactionTrigger::ThresholdExceeded => {
            ContextCompactionTrigger::ThresholdExceeded
        }
        SessionHistoryCompactionTrigger::HardThresholdExceeded => {
            ContextCompactionTrigger::HardThresholdExceeded
        }
        SessionHistoryCompactionTrigger::BudgetExceeded => ContextCompactionTrigger::BudgetExceeded,
        SessionHistoryCompactionTrigger::HistorySectionsDropped => {
            ContextCompactionTrigger::HistorySectionsDropped
        }
    }
}

pub(super) fn map_context_compaction_outcome(
    outcome: SessionHistoryCompactionOutcome,
) -> ContextCompactionOutcome {
    match outcome {
        SessionHistoryCompactionOutcome::Applied => ContextCompactionOutcome::Applied,
        SessionHistoryCompactionOutcome::RejectedMissingSummary => {
            ContextCompactionOutcome::RejectedMissingSummary
        }
    }
}

pub(super) fn context_compaction_failure_outcome(error: &TalosError) -> ContextCompactionOutcome {
    match error.error_code() {
        talos_error::ErrorCode::ServiceUnavailable
        | talos_error::ErrorCode::ProviderUnavailable
        | talos_error::ErrorCode::Timeout
        | talos_error::ErrorCode::ToolExecutionFailed => {
            ContextCompactionOutcome::FailedProviderError
        }
        talos_error::ErrorCode::InvalidRequest
        | talos_error::ErrorCode::InvalidRetryRequest
        | talos_error::ErrorCode::Unauthenticated
        | talos_error::ErrorCode::PermissionDenied
        | talos_error::ErrorCode::ResourceNotFound
        | talos_error::ErrorCode::SessionNotFound
        | talos_error::ErrorCode::ResourceExpired
        | talos_error::ErrorCode::Conflict
        | talos_error::ErrorCode::ActiveTurnRunning
        | talos_error::ErrorCode::MessageQueueFull
        | talos_error::ErrorCode::MessageQueueBusy
        | talos_error::ErrorCode::RetrySourceCompleted
        | talos_error::ErrorCode::RetrySourceMismatch
        | talos_error::ErrorCode::IdempotencyConflict
        | talos_error::ErrorCode::InvalidCursor
        | talos_error::ErrorCode::EventCursorExpired
        | talos_error::ErrorCode::FsDisabled
        | talos_error::ErrorCode::InvalidPath
        | talos_error::ErrorCode::PathOutsideRoot
        | talos_error::ErrorCode::SymlinkNotSupported
        | talos_error::ErrorCode::PathNotFound
        | talos_error::ErrorCode::NotDirectory
        | talos_error::ErrorCode::InvalidName
        | talos_error::ErrorCode::AlreadyExists
        | talos_error::ErrorCode::UploadTooLarge
        | talos_error::ErrorCode::UnsupportedMediaType
        | talos_error::ErrorCode::AttachmentLimitExceeded
        | talos_error::ErrorCode::UnsupportedAttachment
        | talos_error::ErrorCode::FileInputRejected
        | talos_error::ErrorCode::ContextWindowExceeded
        | talos_error::ErrorCode::SessionBudgetExceeded
        | talos_error::ErrorCode::UsageLimitExceeded
        | talos_error::ErrorCode::ContextCompactionFailed
        | talos_error::ErrorCode::Cancelled
        | talos_error::ErrorCode::RunForcedAbort
        | talos_error::ErrorCode::RunCleanupUnconfirmed
        | talos_error::ErrorCode::InternalError => ContextCompactionOutcome::FailedInternalError,
    }
}

pub(super) fn build_context_compaction_metrics(
    compaction: &SessionHistoryCompaction,
) -> ContextCompactionMetrics {
    ContextCompactionMetrics {
        covers_from_seq: compaction.covers_from_seq,
        covers_until_seq: compaction.covers_until_seq,
        pre_compaction_total_tokens: saturating_u32(compaction.pre_compaction_total_tokens),
        post_compaction_total_tokens: saturating_u32(compaction.post_compaction_total_tokens),
        compaction_input_tokens: compaction.compaction_input_tokens,
        compaction_output_tokens: compaction.compaction_output_tokens,
        trimmed_context_unit_count: compaction.trimmed_context_unit_count,
        raw_tail_preferred_count: Some(saturating_u32(compaction.recent_messages_after)),
        ..Default::default()
    }
}

pub(super) fn token_usage_from_session_history_compaction(
    compaction: Option<&SessionHistoryCompaction>,
) -> TokenUsage {
    let Some(compaction) = compaction else {
        return TokenUsage::default();
    };
    TokenUsage {
        input_tokens: compaction.compaction_input_tokens,
        output_tokens: compaction.compaction_output_tokens,
        total_tokens: compaction.compaction_total_tokens,
        cache_creation_input_tokens: compaction.compaction_cache_creation_input_tokens,
        cache_read_input_tokens: compaction.compaction_cache_read_input_tokens,
    }
}

pub(super) async fn latest_compact_prompt_cursor(
    session_artifact_store: &dyn crate::KernelSessionArtifactStore,
    kernel_config: &AgentKernelConfig,
    session_id: &str,
) -> TalosResult<Option<talos_prompt::CompactPromptCursor>> {
    let summaries = session_artifact_store
        .load_summaries(
            std::path::Path::new(&kernel_config.workspace_dir),
            session_id,
            1,
        )
        .await?;
    Ok(summaries
        .last()
        .map(|summary| talos_prompt::CompactPromptCursor {
            covers_until_seq: summary.covers_until_seq,
        }))
}

fn saturating_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}
