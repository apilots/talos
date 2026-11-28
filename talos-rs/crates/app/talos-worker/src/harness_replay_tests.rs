use super::*;
use crate::{TalosWorker, TalosWorkerResources};
use talos_agent_api::{
    AgentEvent, AgentEventPayload, AgentMessage, AgentMessageRole, AgentMessageStatus,
    AgentTurnStarted,
};

#[tokio::test]
async fn production_harness_replays_only_committed_commands_without_starting_work() {
    let home = tempfile::tempdir().unwrap();
    let mut config = talos_config::Config::default()
        .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().to_owned()))
        .with_workspace_definition(talos_config::WorkspaceDefinition::new(
            "workspace-1",
            home.path(),
        ));
    config.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: config.llm.model.clone(),
        name: config.llm.model.clone(),
        context_window: 32768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: vec![],
        cost: Default::default(),
    }];
    // Lazy ports deliberately have no server: a journal lookup must not use them.
    let resources = Arc::new(
        agent_client::AgentSessionDataClient::connect(
            "https://127.0.0.1:1",
            agent_client::AgentTlsTrust::SystemRoots,
            NodeAuth {
                node_id: "worker".into(),
                token: "test-only".into(),
            },
        )
        .await
        .unwrap(),
    );
    let worker = TalosWorker::build(
        config.clone(),
        TalosWorkerResources {
            attachment_reader: resources.clone(),
            tool_image_store: resources,
            agent_control: None,
            automation_control: None,
        },
    )
    .await
    .unwrap();
    let (agent_id, harness) = &worker.harnesses()[0];
    let model_selection = talos_types::resolve_default_model_selection(&config.llm).unwrap();
    let model = talos_types::resolve_model_catalog(&config.llm)
        .into_iter()
        .find(|model| {
            model.provider == model_selection.model_ref.provider
                && model.model.id == model_selection.model_ref.id
        })
        .unwrap();
    model_selection.resolve_reasoning(&model).unwrap();
    let scope = AgentSessionContext {
        agent_id: agent_id.clone(),
        session_id: "journal-replay".into(),
        workspace: config.resolve_workspace("workspace-1", "user-1").unwrap(),
    };
    let mut params = AgentRunParams {
        subagent_limit: None,
        command: None,
        automation_origin: None,
        context: AgentContext {
            tree_identity: talos_types::SessionTreeIdentity::Primary,
            history_cursor: None,
            session: scope.clone(),
            turn_id: "turn-1".into(),
            project_id: None,
            working_directory: None,
            environment: None,
            model,
            model_selection,
            permission_mode: Default::default(),
        },
        input: AgentMessage {
            message_id: "input-1".into(),
            turn_id: "turn-1".into(),
            role: AgentMessageRole::User,
            status: AgentMessageStatus::Completed,
            blocks: vec![],
            created_at: 1,
            completed_at: Some(1),
        },
        checkpoint: None,
        continuation: None,
    };
    let store = worker.execution_store();
    store
        .admit(
            &scope,
            &AgentExecutionCommand::Run {
                params: Box::new(params.clone()),
            },
            vec![AgentEvent {
                identity: params.context.identity(),
                sequence: 1,
                occurred_at: 1,
                payload: AgentEventPayload::TurnStarted(AgentTurnStarted {}),
            }],
            &AgentRunCheckpoint {
                last_sequence: 1,
                messages: vec![],
                metrics: Default::default(),
            },
        )
        .await
        .unwrap();
    // Match the effective owner context even if the caller supplies a different path.
    params.context.session.workspace.root = home.path().join("untrusted-root");
    let replay = harness.replay_admitted_run(params.clone()).await.unwrap();
    assert!(replay.is_some());
    drop(replay);
    params.input.message_id = "new-input".into();
    assert!(harness.replay_admitted_run(params).await.unwrap().is_none());
    assert_eq!(
        store
            .read_events(AgentSessionEventsParams {
                session: scope,
                after_cursor: None,
                limit: 1,
            })
            .await
            .unwrap()
            .events
            .len(),
        1
    );
    worker.shutdown().await.unwrap();
}
