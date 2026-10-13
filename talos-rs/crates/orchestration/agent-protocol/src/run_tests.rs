use talos_agent_api::{
    AgentContinuationRef, AgentMessage, AgentMessageRole, AgentMessageStatus, AgentRunMetrics,
};
use talos_config::{EffectiveWorkspace, ProviderModelConfig};
use talos_types::{ModelInfo, ModelRef, ModelSelection, UserPermissionMode};

use crate::{AgentContext, AgentRunCheckpoint, AgentRunParams, AgentSessionContext};

#[test]
fn continuation_requires_checkpoint_and_provider_reference_together() {
    let mut params = run_params();
    params.continuation = Some(AgentContinuationRef {
        opaque_id: "continuation-1".to_owned(),
    });

    assert!(params.validate().is_err());

    params.checkpoint = Some(AgentRunCheckpoint {
        last_sequence: 1,
        messages: Vec::new(),
        metrics: AgentRunMetrics::default(),
    });
    assert!(params.validate().is_ok());
}

#[test]
fn input_must_be_one_completed_user_message_for_the_same_turn() {
    let mut params = run_params();
    params.input.role = AgentMessageRole::Agent;
    assert!(params.validate().is_err());

    params.input.role = AgentMessageRole::User;
    params.input.turn_id = "another-turn".to_owned();
    assert!(params.validate().is_err());
}

fn run_params() -> AgentRunParams {
    AgentRunParams {
        context: AgentContext {
            history_cursor: None,
            session: AgentSessionContext {
                agent_id: "default".to_owned(),
                session_id: "session-1".to_owned(),
                workspace: EffectiveWorkspace {
                    workspace_id: "workspace-1".to_owned(),
                    user_id: "user-1".to_owned(),
                    root: "/tmp/workspace-1".into(),
                },
            },
            turn_id: "turn-1".to_owned(),
            project_id: None,
            working_directory: None,
            environment: None,
            model: ModelInfo::from_provider_model(
                "test".to_owned(),
                ProviderModelConfig {
                    id: "model".to_owned(),
                    name: "Model".to_owned(),
                    context_window: 32_768,
                    reasoning: false,
                    reasoning_effort: Default::default(),
                    speed: Default::default(),
                    input: Vec::new(),
                    cost: Default::default(),
                },
            ),
            model_selection: ModelSelection::without_reasoning(ModelRef {
                provider: "test".to_owned(),
                id: "model".to_owned(),
            }),
            permission_mode: UserPermissionMode::FullAccess,
        },
        input: AgentMessage {
            message_id: "message-user-1".to_owned(),
            turn_id: "turn-1".to_owned(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: Vec::new(),
            created_at: 1,
            completed_at: Some(1),
        },
        checkpoint: None,
        continuation: None,
    }
}
