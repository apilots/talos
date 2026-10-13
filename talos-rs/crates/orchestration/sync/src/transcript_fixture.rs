use talos_agent_api::*;
use talos_types::*;

use crate::{SessionExecutionBinding, SyncSessionSnapshot};

pub(super) fn session_and_events(
    computer_id: &str,
) -> (SyncSessionSnapshot, Vec<TimelineEventPayload>) {
    let identity = AgentTurnRef::new("session-1", "turn-1");
    let session = SyncSessionSnapshot {
        session: SessionInfo {
            node_id: computer_id.into(),
            id: identity.session_id.clone(),
            title: "Tool result history".into(),
            user_id: "account-1".into(),
            source_type: SessionSourceType::Desktop,
            source_provider: SessionSourceProvider::Sdk,
            model_selection: ModelSelection::without_reasoning(ModelRef {
                provider: "original".into(),
                id: "configured".into(),
            }),
            configured_default_mode: UserPermissionMode::AskForApproval,
            effective_mode: UserPermissionMode::AskForApproval,
            created_at: 1,
            last_activity: 101,
            message_count: 1,
            usage: SessionUsage::default(),
            project_id: None,
            current_directory: Some("/worker/work".into()),
        },
        conf: SessionConf {
            model_selection: None,
            user_permission_mode: Some(UserPermissionMode::AskForApproval),
        },
        environment: Some(TalosSessionEnvironment {
            directory: "/worker/work".into(),
            connection_mode: TalosConnectionMode::Embedded,
        }),
        channel_id: Some("desktop".into()),
        thread_id: Some("original-thread".into()),
        binding: SessionExecutionBinding {
            computer_id: computer_id.into(),
            agent_id: "agent-1".into(),
            workspace_id: "session-1".into(),
            workspace_user_id: "account-1".into(),
        },
        tree_identity: SessionTreeIdentity::Primary,
        memory_processed_through: None,
    };
    let mut blocks = vec![
        AgentMessageBlock {
            block_id: "call".into(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::ToolCall(AgentToolCall {
                call_id: "call-1".into(),
                name: "Read".into(),
                arguments: AgentToolCallArguments::Complete {
                    value: ToolArguments::new(
                        serde_json::json!({"path":"/worker/file", "nested": {"numbers": [1, 2.5], "optional": null}}),
                    ),
                },
                progress: vec![],
            }),
            started_at: 100,
            completed_at: Some(101),
        },
        AgentMessageBlock {
            block_id: "result".into(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::ToolResult(AgentToolResult {
                call_id: "call-1".into(),
                tool_name: "Read".into(),
                content: vec![
                    TalosContentItem::Json {
                        value: JsonContentValue::new(serde_json::json!(["array", {"value": 42}])),
                    },
                    TalosContentItem::Text {
                        text: "original result".into(),
                    },
                ],
                is_error: false,
            }),
            started_at: 100,
            completed_at: Some(101),
        },
    ];
    // Original attachment, media and selected-question fields must survive
    // generated schema conversion and the transactional Loro import unchanged.
    for (index, content) in [
        AgentUserContent::File {
            file_id: "source-file".into(),
        },
        AgentUserContent::Audio {
            source: AgentMediaSource::Url {
                url: "https://example.test/audio".into(),
            },
            mime_type: Some("audio/wav".into()),
        },
        AgentUserContent::Video {
            source: AgentMediaSource::InlineBase64 {
                data: "AA==".into(),
            },
            mime_type: Some("video/mp4".into()),
        },
        AgentUserContent::UserActionResponse {
            action_id: "question-action".into(),
            response: UserActionResponse::Question(QuestionAnswer::SelectedOption {
                option_id: "plan".into(),
            }),
        },
    ]
    .into_iter()
    .enumerate()
    {
        blocks.push(AgentMessageBlock {
            block_id: format!("user-{index}"),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::UserContent(content),
            started_at: 100,
            completed_at: Some(101),
        });
    }
    let payloads = [
        AgentEventPayload::TurnStarted(AgentTurnStarted {}),
        AgentEventPayload::MessageCompleted(AgentMessage {
            message_id: "message-1".into(),
            turn_id: identity.turn_id.clone(),
            role: AgentMessageRole::Agent,
            status: AgentMessageStatus::Completed,
            blocks,
            created_at: 100,
            completed_at: Some(101),
        }),
        AgentEventPayload::TurnFailed(AgentTurnFailed {
            error: talos_error::ApiError::with_message(
                talos_error::ErrorCode::ProviderUnavailable,
                "Provider became unavailable",
            ),
            retryable: true,
        }),
    ];
    let events = payloads
        .into_iter()
        .enumerate()
        .map(|(index, payload)| {
            TimelineEventPayload::Agent(AgentEvent {
                identity: identity.clone(),
                sequence: index as u64 + 1,
                occurred_at: 100 + index as i64,
                payload,
            })
        })
        .collect();
    (session, events)
}
