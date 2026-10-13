use agent_pb as pb;
use talos_agent_api::{AgentContextCompaction, AgentContextCompactionDelta};
use talos_error::TalosResult;
use talos_types::{
    ContextCompactionMetrics, ContextCompactionOutcome, ContextCompactionStage,
    ContextCompactionTrigger,
};

use super::invalid;

pub fn encode_compaction(value: AgentContextCompaction) -> pb::ContextCompaction {
    pb::ContextCompaction {
        trigger: encode_trigger(value.trigger),
        stage: encode_stage(value.stage),
        outcome: value.outcome.map(encode_outcome),
        metrics: value.metrics.map(encode_metrics),
        message: value.message,
    }
}

pub fn decode_compaction(value: pb::ContextCompaction) -> TalosResult<AgentContextCompaction> {
    Ok(AgentContextCompaction {
        trigger: decode_trigger(value.trigger)?,
        stage: decode_stage(value.stage)?,
        outcome: value.outcome.map(decode_outcome).transpose()?,
        metrics: value.metrics.map(decode_metrics),
        message: value.message,
    })
}

pub fn encode_compaction_delta(value: AgentContextCompactionDelta) -> pb::CompactionDelta {
    pb::CompactionDelta {
        stage: value.stage.map(encode_stage),
        outcome: value.outcome.map(encode_outcome),
        metrics: value.metrics.map(encode_metrics),
        message: value.message,
    }
}

pub fn decode_compaction_delta(
    value: pb::CompactionDelta,
) -> TalosResult<AgentContextCompactionDelta> {
    Ok(AgentContextCompactionDelta {
        stage: value.stage.map(decode_stage).transpose()?,
        outcome: value.outcome.map(decode_outcome).transpose()?,
        metrics: value.metrics.map(decode_metrics),
        message: value.message,
    })
}

fn encode_metrics(value: ContextCompactionMetrics) -> pb::CompactionMetrics {
    pb::CompactionMetrics {
        covers_from_seq: value.covers_from_seq,
        covers_until_seq: value.covers_until_seq,
        pre_compaction_total_tokens: value.pre_compaction_total_tokens,
        post_compaction_total_tokens: value.post_compaction_total_tokens,
        compaction_input_tokens: value.compaction_input_tokens,
        compaction_output_tokens: value.compaction_output_tokens,
        trimmed_context_unit_count: value.trimmed_context_unit_count,
        raw_tail_preferred_count: value.raw_tail_preferred_count,
        raw_tail_backfilled_count: value.raw_tail_backfilled_count,
        working_context_input_messages: value.working_context_input_messages,
        working_context_compacted_messages: value.working_context_compacted_messages,
        working_context_preserved_recent_exchanges: value
            .working_context_preserved_recent_exchanges,
    }
}

fn decode_metrics(value: pb::CompactionMetrics) -> ContextCompactionMetrics {
    ContextCompactionMetrics {
        covers_from_seq: value.covers_from_seq,
        covers_until_seq: value.covers_until_seq,
        pre_compaction_total_tokens: value.pre_compaction_total_tokens,
        post_compaction_total_tokens: value.post_compaction_total_tokens,
        compaction_input_tokens: value.compaction_input_tokens,
        compaction_output_tokens: value.compaction_output_tokens,
        trimmed_context_unit_count: value.trimmed_context_unit_count,
        raw_tail_preferred_count: value.raw_tail_preferred_count,
        raw_tail_backfilled_count: value.raw_tail_backfilled_count,
        working_context_input_messages: value.working_context_input_messages,
        working_context_compacted_messages: value.working_context_compacted_messages,
        working_context_preserved_recent_exchanges: value
            .working_context_preserved_recent_exchanges,
    }
}

fn encode_trigger(value: ContextCompactionTrigger) -> i32 {
    match value {
        ContextCompactionTrigger::UserRequested => pb::CompactionTrigger::UserRequested as i32,
        ContextCompactionTrigger::ThresholdExceeded => {
            pb::CompactionTrigger::ThresholdExceeded as i32
        }
        ContextCompactionTrigger::HardThresholdExceeded => {
            pb::CompactionTrigger::HardThresholdExceeded as i32
        }
        ContextCompactionTrigger::BudgetExceeded => pb::CompactionTrigger::BudgetExceeded as i32,
        ContextCompactionTrigger::HistorySectionsDropped => {
            pb::CompactionTrigger::HistorySectionsDropped as i32
        }
        ContextCompactionTrigger::WorkingContextExceeded => {
            pb::CompactionTrigger::WorkingContextExceeded as i32
        }
    }
}

fn decode_trigger(value: i32) -> TalosResult<ContextCompactionTrigger> {
    match pb::CompactionTrigger::try_from(value) {
        Ok(pb::CompactionTrigger::UserRequested) => Ok(ContextCompactionTrigger::UserRequested),
        Ok(pb::CompactionTrigger::ThresholdExceeded) => {
            Ok(ContextCompactionTrigger::ThresholdExceeded)
        }
        Ok(pb::CompactionTrigger::HardThresholdExceeded) => {
            Ok(ContextCompactionTrigger::HardThresholdExceeded)
        }
        Ok(pb::CompactionTrigger::BudgetExceeded) => Ok(ContextCompactionTrigger::BudgetExceeded),
        Ok(pb::CompactionTrigger::HistorySectionsDropped) => {
            Ok(ContextCompactionTrigger::HistorySectionsDropped)
        }
        Ok(pb::CompactionTrigger::WorkingContextExceeded) => {
            Ok(ContextCompactionTrigger::WorkingContextExceeded)
        }
        _ => Err(invalid()),
    }
}

fn encode_stage(value: ContextCompactionStage) -> i32 {
    match value {
        ContextCompactionStage::Preparing => pb::CompactionStage::Preparing as i32,
        ContextCompactionStage::Summarizing => pb::CompactionStage::Summarizing as i32,
        ContextCompactionStage::RebuildingPrompt => pb::CompactionStage::RebuildingPrompt as i32,
        ContextCompactionStage::Trimming => pb::CompactionStage::Trimming as i32,
        ContextCompactionStage::Completed => pb::CompactionStage::Completed as i32,
        ContextCompactionStage::Failed => pb::CompactionStage::Failed as i32,
    }
}

fn decode_stage(value: i32) -> TalosResult<ContextCompactionStage> {
    match pb::CompactionStage::try_from(value) {
        Ok(pb::CompactionStage::Preparing) => Ok(ContextCompactionStage::Preparing),
        Ok(pb::CompactionStage::Summarizing) => Ok(ContextCompactionStage::Summarizing),
        Ok(pb::CompactionStage::RebuildingPrompt) => Ok(ContextCompactionStage::RebuildingPrompt),
        Ok(pb::CompactionStage::Trimming) => Ok(ContextCompactionStage::Trimming),
        Ok(pb::CompactionStage::Completed) => Ok(ContextCompactionStage::Completed),
        Ok(pb::CompactionStage::Failed) => Ok(ContextCompactionStage::Failed),
        _ => Err(invalid()),
    }
}

fn encode_outcome(value: ContextCompactionOutcome) -> i32 {
    match value {
        ContextCompactionOutcome::Applied => pb::CompactionOutcome::Applied as i32,
        ContextCompactionOutcome::NoChanges => pb::CompactionOutcome::NoChanges as i32,
        ContextCompactionOutcome::RejectedMissingSummary => {
            pb::CompactionOutcome::RejectedMissingSummary as i32
        }
        ContextCompactionOutcome::TrimmedFallbackApplied => {
            pb::CompactionOutcome::TrimmedFallbackApplied as i32
        }
        ContextCompactionOutcome::FailedContextStillTooLarge => {
            pb::CompactionOutcome::FailedContextStillTooLarge as i32
        }
        ContextCompactionOutcome::FailedProviderError => {
            pb::CompactionOutcome::FailedProviderError as i32
        }
        ContextCompactionOutcome::FailedInternalError => {
            pb::CompactionOutcome::FailedInternalError as i32
        }
    }
}

fn decode_outcome(value: i32) -> TalosResult<ContextCompactionOutcome> {
    match pb::CompactionOutcome::try_from(value) {
        Ok(pb::CompactionOutcome::Applied) => Ok(ContextCompactionOutcome::Applied),
        Ok(pb::CompactionOutcome::NoChanges) => Ok(ContextCompactionOutcome::NoChanges),
        Ok(pb::CompactionOutcome::RejectedMissingSummary) => {
            Ok(ContextCompactionOutcome::RejectedMissingSummary)
        }
        Ok(pb::CompactionOutcome::TrimmedFallbackApplied) => {
            Ok(ContextCompactionOutcome::TrimmedFallbackApplied)
        }
        Ok(pb::CompactionOutcome::FailedContextStillTooLarge) => {
            Ok(ContextCompactionOutcome::FailedContextStillTooLarge)
        }
        Ok(pb::CompactionOutcome::FailedProviderError) => {
            Ok(ContextCompactionOutcome::FailedProviderError)
        }
        Ok(pb::CompactionOutcome::FailedInternalError) => {
            Ok(ContextCompactionOutcome::FailedInternalError)
        }
        _ => Err(invalid()),
    }
}
