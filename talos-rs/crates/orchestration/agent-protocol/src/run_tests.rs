use talos_agent_api::{
    AgentMessage, AgentMessageBlock, AgentMessageBlockContent, AgentMessageBlockStatus,
    AgentMessageRole, AgentMessageStatus, AgentUserContent,
};
use talos_config::{EffectiveWorkspace, ProviderModelConfig};
use talos_types::{ModelInfo, ModelRef, ModelSelection, UserPermissionMode};

use crate::{AgentContext, AgentRunIntent, AgentRunParams, AgentSessionContext};

#[test]
fn responding_requires_one_identified_user_action() {
    let mut params = run_params();
    params.intent = AgentRunIntent::RespondUserAction;
    assert!(params.validate().is_err());
    params.input.blocks = vec![response()];
    params.validate().unwrap();
    assert_eq!(params.user_action_response().unwrap().0, "action-1");
    params.input.blocks.push(response());
    assert!(params.validate().is_err());
    params.input.blocks.truncate(1);
    if let AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
        action_id,
        ..
    }) = &mut params.input.blocks[0].content
    {
        action_id.clear();
    }
    assert!(params.validate().is_err());
}

#[test]
fn starting_cannot_bypass_the_user_action_intent() {
    let mut params = run_params();
    params.input.blocks = vec![response()];
    assert!(params.validate().is_err());
}

#[test]
fn worker_intents_survive_protobuf_without_provider_restore_inputs() {
    let config = talos_config::Config::default().with_workspace_definition(
        talos_config::WorkspaceDefinition::new("workspace-1", "/tmp/workspace-1"),
    );
    for intent in [AgentRunIntent::Start, AgentRunIntent::RespondUserAction] {
        let mut params = run_params();
        params.intent = intent;
        params.context.session.workspace =
            config.resolve_workspace("workspace-1", "user-1").unwrap();
        if intent == AgentRunIntent::RespondUserAction {
            params.input.blocks = vec![response()];
        }
        for private in ["checkpoint", "continuation"] {
            let mut rejected = serde_json::to_value(&params).unwrap();
            rejected
                .as_object_mut()
                .unwrap()
                .insert(private.into(), serde_json::Value::Null);
            assert!(serde_json::from_value::<AgentRunParams>(rejected).is_err());
        }
        let wire = crate::harness_pb::encode_run_params(params.clone()).unwrap();
        assert_eq!(
            crate::harness_pb::decode_run_params(wire.clone(), &config).unwrap(),
            params
        );
        for invalid in [0, 999] {
            let mut malformed = wire.clone();
            malformed.intent = invalid;
            assert!(crate::harness_pb::decode_run_params(malformed, &config)
                .unwrap_err()
                .is_code(talos_error::ErrorCode::InvalidRequest));
        }
    }
}

fn response() -> AgentMessageBlock {
    AgentMessageBlock {
        block_id: "response-1".into(),
        status: AgentMessageBlockStatus::Completed,
        content: AgentMessageBlockContent::UserContent(AgentUserContent::UserActionResponse {
            action_id: "action-1".into(),
            response: talos_types::UserActionResponse::SandboxApproval(
                talos_types::SandboxApprovalDecision::AllowOnce,
            ),
        }),
        started_at: 1,
        completed_at: Some(1),
    }
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
        subagent_limit: None,
        command: None,
        automation_origin: None,
        context: AgentContext {
            tree_identity: talos_types::SessionTreeIdentity::Primary,
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
                    reasoning_replay: Default::default(),
                    reasoning_efforts: None,
                    request_parameters: Default::default(),
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
        intent: AgentRunIntent::Start,
    }
}
