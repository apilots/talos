use super::*;
use talos_types::{AgentMediaSource, UserActionResponse};

#[test]
fn canonical_message_round_trips_without_untyped_payloads() {
    let expected = AgentMessage {
        message_id: "message-1".to_string(),
        turn_id: "turn-1".to_string(),
        role: AgentMessageRole::Agent,
        status: AgentMessageStatus::Completed,
        blocks: vec![AgentMessageBlock {
            block_id: "block-1".to_string(),
            status: AgentMessageBlockStatus::Completed,
            content: AgentMessageBlockContent::AssistantText(AgentAssistantText {
                phase: AgentAssistantTextPhase::FinalAnswer,
                text: "done".to_string(),
            }),
            started_at: 100,
            completed_at: Some(101),
        }],
        created_at: 100,
        completed_at: Some(101),
    };

    let encoded = serde_json::to_value(&expected).expect("serialize canonical message");
    let decoded: AgentMessage =
        serde_json::from_value(encoded).expect("deserialize canonical message");

    assert_eq!(decoded, expected);
}

#[test]
fn user_action_response_is_structured_user_content() {
    let content = AgentUserContent::UserActionResponse {
        action_id: "action-1".to_string(),
        response: UserActionResponse::Question(talos_types::QuestionAnswer::FreeForm {
            text: "continue".to_string(),
        }),
    };

    let encoded = serde_json::to_value(&content).expect("serialize UserAction response");

    assert_eq!(encoded["type"], "userActionResponse");
    assert_eq!(encoded["actionId"], "action-1");
    assert!(encoded.get("action_id").is_none());
    assert_eq!(encoded["response"]["type"], "question");
}

#[test]
fn user_content_variant_fields_use_the_protocol_casing() {
    let content = AgentUserContent::Audio {
        source: AgentMediaSource::InlineBase64 {
            data: "audio".to_string(),
        },
        mime_type: Some("audio/wav".to_string()),
    };

    let encoded = serde_json::to_value(&content).expect("serialize Agent User content");

    assert_eq!(encoded["mimeType"], "audio/wav");
    assert!(encoded.get("mime_type").is_none());
}

#[test]
fn prompt_warning_survives_history_serialization_with_typed_parameters() {
    let notice = AgentSystemNotice {
        level: TalosTimelineNoticeLevel::Warning,
        content: SystemNoticeContent::PromptWarning(PromptWarningNotice {
            message: "Prompt bootstrap file was truncated: IDENTITY.md (10 of 30 characters)"
                .into(),
            warning: PromptWarning::BootstrapFileTruncated {
                file: PromptWarningFile::Identity,
                injected_chars: 10,
                raw_chars: 30,
            },
        }),
    };
    let encoded = serde_json::to_value(&notice).unwrap();
    assert_eq!(
        encoded,
        serde_json::json!({
            "level": "warning", "content": { "type": "promptWarning", "data": {
                "message": notice.message(),
                "warning": { "type": "bootstrapFileTruncated", "file": "identity", "injectedChars": 10, "rawChars": 30 }
            } }
        })
    );
    assert_eq!(
        serde_json::from_value::<AgentSystemNotice>(encoded).unwrap(),
        notice
    );
}
