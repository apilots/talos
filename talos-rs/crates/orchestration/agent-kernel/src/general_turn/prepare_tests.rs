use super::prepare::*;
use super::prepare_compaction::context_compaction_failure_outcome;
use super::types::ToolCallRecord;
use super::*;

use crate::{KernelWarning, PromptDynamicSectionDroppedWarning};
use serde_json::json;
use talos_error::TalosError;
use talos_prompt::PromptSectionKind;
use talos_types::talos::ContextCompactionOutcome;

fn build_executed_turn_artifacts(
    tool_call_log: Vec<ToolCallRecord>,
    tool_result_log: Vec<ToolResultLog>,
) -> ExecutedTurnArtifacts {
    ExecutedTurnArtifacts {
        output_text: "ok".to_string(),
        output_items: Vec::new(),
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage::default(),
        last_model_usage: TokenUsage::default(),
        metrics: TurnExecutionMetrics::default(),
        reasoning_log: Vec::new(),
        tool_call_log,
        tool_result_log,
        pending_user_action: None,
        final_answer_streamed: false,
        execution_summary_streamed: false,
    }
}

#[test]
fn test_build_session_continuity_records_recent_goal_failure_and_retry_guidance() {
    let executed = build_executed_turn_artifacts(
        vec![ToolCallRecord {
            call_id: "call-1".to_string(),
            name: "Bash".to_string(),
            arguments: json!({"path": "/tmp/project/src/lib.rs"}).into(),
        }],
        vec![ToolResultLog {
            call_id: "call-1".to_string(),
            tool: "Bash".to_string(),
            payload: json!({"error": "permission denied"}),
            is_error: true,
        }],
    );

    let continuity = build_session_continuity("Inspect the failing file update flow.", &executed)
        .expect("continuity should be built");

    assert_eq!(
        continuity.recent_goal,
        "Inspect the failing file update flow."
    );
    assert_eq!(
        continuity.last_failure.as_deref(),
        Some("Bash returned an error")
    );
    assert_eq!(
        continuity.suggested_next_step.as_deref(),
        Some("adjust the failing path, command, or tool arguments before retrying")
    );
    assert_eq!(
        continuity.pending_tasks,
        vec!["fix the failing tool step and retry".to_string()]
    );
}

#[test]
fn test_build_session_continuity_prefers_verified_evidence_guidance_without_failure() {
    let executed = build_executed_turn_artifacts(
        vec![ToolCallRecord {
            call_id: "call-1".to_string(),
            name: "Read".to_string(),
            arguments: json!({"path": "/tmp/project/Cargo.toml"}).into(),
        }],
        vec![ToolResultLog {
            call_id: "call-1".to_string(),
            tool: "Read".to_string(),
            payload: json!({"content": "[package]"}),
            is_error: false,
        }],
    );

    let continuity = build_session_continuity("Summarize the package setup.", &executed)
        .expect("continuity should be built");

    assert_eq!(continuity.recent_goal, "Summarize the package setup.");
    assert_eq!(continuity.last_failure, None);
    assert_eq!(
        continuity.suggested_next_step.as_deref(),
        Some("answer from the verified evidence without re-reading the same paths")
    );
    assert_eq!(
        continuity.pending_tasks,
        vec!["complete the answer from the verified evidence".to_string()]
    );
}

#[test]
fn test_prompt_warning_key_includes_dynamic_dropped_sections() {
    let warning = KernelWarning::PromptDynamicSectionDropped(PromptDynamicSectionDroppedWarning {
        sections: vec![
            PromptSectionKind::ExpandedToolCatalog,
            PromptSectionKind::RecentMessages,
        ],
    });

    assert_eq!(
        prompt_warning_key(&warning).as_deref(),
        Some("dynamic_dropped:ExpandedToolCatalog,RecentMessages")
    );
}

#[test]
fn test_context_compaction_failure_outcome_separates_provider_and_internal_errors() {
    assert_eq!(
        context_compaction_failure_outcome(&TalosError::public(
            talos_error::ErrorCode::ToolExecutionFailed,
            "parse summary json after retries".to_string()
        )),
        ContextCompactionOutcome::FailedProviderError
    );
    assert_eq!(
        context_compaction_failure_outcome(&TalosError::internal("save summary".to_string())),
        ContextCompactionOutcome::FailedInternalError
    );
}
