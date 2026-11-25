use talos_agent::AgentEventPipeline;
use talos_agent_api::{
    AgentEventPayload, AgentMessageBlockContent, AgentMessageBlockStatus, AgentPendingInput,
    AgentTurnRef, AgentTurnStarted,
};
use talos_agent_kernel::{
    AgentKernelEventPayload, AssistantTextCommittedStreamData, AssistantTextCompletedStreamData,
    AssistantTextDeltaStreamData, AssistantTextStartedStreamData, KernelStreamItemKey,
    ToolCallStartStreamData, ToolExecutingStreamData, ToolProgressDeltaStreamData,
    ToolQueuedStreamData,
};
use talos_types::{
    ContextCompactionOutcome, ContextCompactionStage, ContextCompactionTrigger,
    DynamicToolResultParams, PendingUserAction, ToolArguments, UserActionParams, UserActionState,
};

use super::*;

#[tokio::test]
async fn bootstrap_warning_keeps_its_kind_without_exposing_server_paths() {
    let (pipeline, sink, _events) = AgentEventPipeline::fresh(AgentTurnRef::new("s", "t"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mapper = KernelAgentEventMapper::new(sink);
    let warning = talos_agent_kernel::KernelWarning::PromptBootstrapFileMissing(
        talos_agent_kernel::PromptBootstrapFileMissingWarning {
            file_kind: talos_prompt::WorkspaceBootstrapFileKind::Identity,
            path: "/private/server/secret/IDENTITY.md".into(),
        },
    );
    mapper
        .map_notice(prompt_warning_notice(warning).unwrap())
        .await
        .unwrap();
    let checkpoint = pipeline.checkpoint().await;
    assert_eq!(
        checkpoint.messages[0].blocks[0].content,
        AgentMessageBlockContent::SystemNotice(AgentSystemNotice {
            level: TalosTimelineNoticeLevel::Warning,
            content: talos_agent_api::SystemNoticeContent::PromptWarning(
                talos_agent_api::PromptWarningNotice {
                    message: "Prompt bootstrap file is missing: IDENTITY.md".into(),
                    warning: talos_agent_api::PromptWarning::BootstrapFileMissing {
                        file: talos_agent_api::PromptWarningFile::Identity
                    },
                }
            ),
        })
    );
}

#[tokio::test]
async fn assistant_kernel_lifecycle_maps_to_one_canonical_agent_block() {
    let (pipeline, sink, _events) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mut mapper = KernelAgentEventMapper::new(sink);
    let key = KernelStreamItemKey {
        round: Some(1),
        local_id: "answer-1".to_string(),
    };

    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::AssistantTextStarted(AssistantTextStartedStreamData {
                item_key: key.clone(),
                phase: AssistantTextPhase::Unknown,
                round: Some(1),
            }),
        ))
        .await
        .unwrap();
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::AssistantTextDelta(AssistantTextDeltaStreamData {
                item_key: key.clone(),
                delta: "done".to_string(),
            }),
        ))
        .await
        .unwrap();
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::AssistantTextCompleted(AssistantTextCompletedStreamData {
                item_key: key.clone(),
            }),
        ))
        .await
        .unwrap();
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::AssistantTextCommitted(AssistantTextCommittedStreamData {
                item_key: key,
                phase: AssistantTextPhase::FinalAnswer,
            }),
        ))
        .await
        .unwrap();
    mapper.finish_completed().await.unwrap();
    let checkpoint = pipeline.checkpoint().await;
    let block = &checkpoint.messages[0].blocks[0];
    assert_eq!(
        block.content,
        AgentMessageBlockContent::AssistantText(AgentAssistantText {
            phase: AgentAssistantTextPhase::FinalAnswer,
            text: "done".to_string(),
        })
    );
}

#[tokio::test]
async fn tool_execution_keeps_queue_progress_without_synthesizing_a_start_notice() {
    let (pipeline, sink, _events) = AgentEventPipeline::fresh(AgentTurnRef::new("s", "t"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mut mapper = KernelAgentEventMapper::new(sink);
    let stream_key = ToolStreamKey { round: 1, index: 0 };
    for payload in [
        AgentKernelEventPayload::ToolQueued(ToolQueuedStreamData {
            stream_key: stream_key.clone(),
            call_id: "call-1".into(),
            tool_name: "Bash".into(),
        }),
        AgentKernelEventPayload::ToolProgressDelta(ToolProgressDeltaStreamData {
            stream_key: stream_key.clone(),
            call_id: "call-1".into(),
            tool_name: "Bash".into(),
            message: "waiting for execution slot".into(),
            progress_kind: ToolProgressKind::Status,
        }),
        AgentKernelEventPayload::ToolExecuting(ToolExecutingStreamData {
            stream_key,
            call_id: "call-1".into(),
            tool_name: "Bash".into(),
        }),
    ] {
        mapper.map(AgentKernelEvent::new(payload)).await.unwrap();
    }
    let checkpoint = pipeline.checkpoint().await;
    let blocks = &checkpoint.messages[0].blocks;
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].status, AgentMessageBlockStatus::Running);
    let AgentMessageBlockContent::ToolCall(call) = &blocks[0].content else {
        panic!("expected a tool call");
    };
    assert_eq!(call.progress.len(), 1);
    assert_eq!(call.progress[0].kind, TalosToolProgressKind::Status);
    assert_eq!(call.progress[0].text, "waiting for execution slot");
}

#[tokio::test]
async fn live_input_closes_the_message_after_explicit_phase_text_ends() {
    let (pipeline, sink, _events) = AgentEventPipeline::fresh(AgentTurnRef::new("s", "t"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mut mapper = KernelAgentEventMapper::new(sink);
    let key = KernelStreamItemKey {
        round: Some(1),
        local_id: "text-1".into(),
    };
    for event in [
        AgentKernelEventPayload::AssistantTextStarted(AssistantTextStartedStreamData {
            item_key: key.clone(),
            phase: AssistantTextPhase::Commentary,
            round: Some(1),
        }),
        AgentKernelEventPayload::AssistantTextDelta(AssistantTextDeltaStreamData {
            item_key: key.clone(),
            delta: "Checking the workspace.".into(),
        }),
        AgentKernelEventPayload::AssistantTextCompleted(AssistantTextCompletedStreamData {
            item_key: key,
        }),
        AgentKernelEventPayload::UserInput(talos_agent_kernel::UserInput {
            text: "Continue here.".into(),
            file_ids: vec![],
        }),
    ] {
        mapper.map(AgentKernelEvent::new(event)).await.unwrap();
    }
    let checkpoint = pipeline.checkpoint().await;
    let message = &checkpoint.messages[0];
    assert_eq!(message.status, AgentMessageStatus::Completed);
    assert_eq!(message.blocks[0].status, AgentMessageBlockStatus::Completed);
}

#[tokio::test]
async fn waiting_result_reuses_the_tool_block_as_the_pending_action_identity() {
    let (pipeline, sink, _events) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mut mapper = KernelAgentEventMapper::new(sink);
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::ToolCallStart(ToolCallStartStreamData {
                index: 0,
                call_id: "call-1".to_string(),
                tool_name: "search".to_string(),
                arguments: ToolArguments::from(serde_json::json!({"query": "Talos"})).into(),
                round: 1,
            }),
        ))
        .await
        .unwrap();
    let (_, block_id) = mapper.waiting_handles("call-1").unwrap();
    let block_id = block_id.to_string();

    pipeline
        .record(AgentEventPayload::WaitingForInput(
            AgentPendingInput::UserAction {
                action: PendingUserAction {
                    action_id: "action-1".to_string(),
                    call_id: "call-1".to_string(),
                    item_id: Some(block_id),
                    state: UserActionState::Pending,
                    params: UserActionParams::DynamicToolResult(DynamicToolResultParams {
                        tool_name: "search".to_string(),
                    }),
                },
            },
        ))
        .await
        .unwrap();

    assert_eq!(
        pipeline.checkpoint().await.messages[0].blocks[0].status,
        AgentMessageBlockStatus::WaitingForInput
    );
}

#[tokio::test]
async fn provider_failure_closes_compaction_and_message_lifecycles() {
    let (pipeline, sink, _events) =
        AgentEventPipeline::fresh(AgentTurnRef::new("session-1", "turn-1"));
    pipeline
        .record(AgentEventPayload::TurnStarted(AgentTurnStarted {}))
        .await
        .unwrap();
    let mut mapper = KernelAgentEventMapper::new(sink);
    mapper
        .map(AgentKernelEvent::new(
            AgentKernelEventPayload::ContextCompaction(
                talos_agent_kernel::ContextCompactionStreamData {
                    trigger: ContextCompactionTrigger::UserRequested,
                    stage: ContextCompactionStage::Preparing,
                    outcome: None,
                    metrics: None,
                    message: Some("started".to_string()),
                },
            ),
        ))
        .await
        .unwrap();

    mapper
        .finish_failed(&talos_error::TalosError::new(
            talos_error::ErrorCode::InternalError,
            "compaction failed",
        ))
        .await
        .unwrap();

    let checkpoint = pipeline.checkpoint().await;
    let block = &checkpoint.messages[0].blocks[0];
    let AgentMessageBlockContent::ContextCompaction(content) = &block.content else {
        panic!("expected ContextCompaction block");
    };
    assert_eq!(
        (
            checkpoint.messages[0].status,
            block.status,
            content.stage,
            content.outcome,
            content.message.as_deref(),
        ),
        (
            talos_agent_api::AgentMessageStatus::Failed,
            AgentMessageBlockStatus::Failed,
            ContextCompactionStage::Failed,
            Some(ContextCompactionOutcome::FailedInternalError),
            Some("compaction failed"),
        )
    );
}
