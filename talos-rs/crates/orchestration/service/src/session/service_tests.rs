use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use talos_agent::{AgentContext, AgentSessionContext};
use talos_agent_api::AgentTurnRef;
use talos_error::{TalosError, TalosResult};
use talos_store::project::{MockProjectStore, ProjectRecord};
use talos_store::session::{
    AccumulateSessionUsageParams, MockSessionStore, PatchSessionParams, SessionQuery,
    SessionRecord, SessionStore, SessionSummary, SessionTreeIdentity,
};
use talos_types::talos::{SessionSource, SessionSourceProvider, SessionSourceType, TokenUsage};
use talos_types::{
    ContextUsage, ModelRef, ModelSelection, SessionConf, SessionPatch, SessionUsage,
    UserPermissionMode,
};

use super::CreateSessionParams;

fn project_record(project_id: &str, user_id: &str) -> ProjectRecord {
    ProjectRecord {
        execution_node_id: "embedded".into(),
        project_id: project_id.to_string(),
        name: "Project".to_string(),
        user_id: user_id.to_string(),
        directory: std::path::PathBuf::from("/tmp/project"),
        status: talos_types::ProjectStatus::default(),
        created_at: 0,
        updated_at: 0,
        session_count: 0,
    }
}

#[derive(Default)]
struct RecordingSessionStore {
    records: Mutex<Vec<SessionRecord>>,
    failing_updates: AtomicUsize,
    history_cursor: Mutex<Option<talos_types::SessionHistoryCursor>>,
}

#[async_trait]
impl SessionStore for RecordingSessionStore {
    async fn get_history_cursor(
        &self,
        _session_id: &str,
    ) -> TalosResult<Option<talos_types::SessionHistoryCursor>> {
        Ok(self.history_cursor.lock().unwrap().clone())
    }
    async fn set_generated_title(
        &self,
        identity: &talos_agent_api::AgentTurnRef,
        title: &str,
    ) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let record = records
            .iter_mut()
            .find(|record| record.id == identity.session_id);
        if let Some(record) = record {
            record.title = title.to_string();
            return Ok(true);
        }
        Ok(false)
    }

    async fn insert_session_record(&self, record: &SessionRecord) -> TalosResult<()> {
        self.records.lock().unwrap().push(record.clone());
        Ok(())
    }

    async fn get_session_record(&self, session_id: &str) -> TalosResult<Option<SessionRecord>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|record| record.id == session_id)
            .cloned())
    }

    async fn get_subagent_record(
        &self,
        tree_root_session_id: &str,
        agent_path: &talos_types::AgentPath,
    ) -> TalosResult<Option<SessionRecord>> {
        Ok(self
            .records
            .lock()
            .unwrap()
            .iter()
            .find(|record| match &record.tree_identity {
                SessionTreeIdentity::Subagent {
                    tree_root_session_id: root,
                    path,
                    ..
                } => root == tree_root_session_id && path == agent_path,
                SessionTreeIdentity::Primary => false,
            })
            .cloned())
    }

    async fn list_session_records(&self, _query: &SessionQuery) -> TalosResult<Vec<SessionRecord>> {
        Ok(self.records.lock().unwrap().clone())
    }

    async fn count_session_records(&self, _query: &SessionQuery) -> TalosResult<usize> {
        Ok(self.records.lock().unwrap().len())
    }

    async fn summarize_sessions(&self, user_id: &str) -> TalosResult<SessionSummary> {
        let records = self.records.lock().unwrap();
        let (session_count, message_count) = records
            .iter()
            .filter(|record| record.user_id == user_id)
            .fold((0_u64, 0_u64), |(count, messages), record| {
                (count + 1, messages + record.message_count)
            });
        Ok(SessionSummary {
            session_count,
            message_count,
        })
    }

    async fn has_active_subagent_turn(&self, _tree_root_session_id: &str) -> TalosResult<bool> {
        Ok(false)
    }

    async fn has_nonterminal_turns(&self, _node_id: Option<&str>) -> TalosResult<bool> {
        Ok(false)
    }

    async fn patch_session(
        &self,
        params: PatchSessionParams,
    ) -> TalosResult<Option<SessionRecord>> {
        if self
            .failing_updates
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(TalosError::internal("injected update failure"));
        }
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records
            .iter_mut()
            .find(|stored| stored.id == params.session_id)
        else {
            return Ok(None);
        };
        stored.conf = params.conf;
        stored.current_directory = params.current_directory;
        stored.last_activity = params.last_activity;
        Ok(Some(stored.clone()))
    }

    async fn accumulate_session_usage(
        &self,
        params: AccumulateSessionUsageParams,
    ) -> TalosResult<Option<SessionRecord>> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records
            .iter_mut()
            .find(|stored| stored.id == params.session_id)
        else {
            return Ok(None);
        };
        stored.usage.tokens.accumulate(&params.tokens);
        stored.last_activity = params.last_activity;
        Ok(Some(stored.clone()))
    }

    async fn clear_session_history(&self, session_id: &str, cleared_at: u64) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(record) = records.iter_mut().find(|record| record.id == session_id) else {
            return Ok(false);
        };
        record.title = talos_types::DEFAULT_SESSION_TITLE.to_string();
        record.last_activity = cleared_at;
        record.message_count = 0;
        record.latest_turn_seq = 0;
        record.memory_processed_until_event_seq = 0;
        record.usage = SessionUsage::default();
        Ok(true)
    }

    async fn update_context_usage(
        &self,
        session_id: &str,
        context_usage: &ContextUsage,
    ) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == session_id) else {
            return Ok(false);
        };
        stored.usage.context = Some(context_usage.clone());
        Ok(true)
    }

    async fn advance_memory_processed_until_event_seq(
        &self,
        session_id: &str,
        _expected_until_seq: u64,
        next_until_seq: u64,
    ) -> TalosResult<()> {
        if let Some(record) = self
            .records
            .lock()
            .unwrap()
            .iter_mut()
            .find(|record| record.id == session_id)
        {
            record.memory_processed_until_event_seq = next_until_seq;
        }
        Ok(())
    }

    async fn delete_session_record(&self, session_id: &str) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let len_before = records.len();
        records.retain(|record| record.id != session_id);
        Ok(records.len() != len_before)
    }

    async fn delete_session_tree(&self, tree_root_session_id: &str) -> TalosResult<bool> {
        let mut records = self.records.lock().unwrap();
        let len_before = records.len();
        records.retain(|record| {
            record.id != tree_root_session_id
                && !matches!(
                    &record.tree_identity,
                    SessionTreeIdentity::Subagent {
                        tree_root_session_id: root,
                        ..
                    } if root == tree_root_session_id
                )
        });
        Ok(records.len() != len_before)
    }
}

fn provider_model(id: &str) -> talos_config::ProviderModelConfig {
    talos_config::ProviderModelConfig {
        reasoning_replay: Default::default(),
        reasoning_efforts: None,
        request_parameters: Default::default(),
        id: id.to_string(),
        name: id.to_string(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }
}

fn config_with_model(provider: &str, model: &str) -> talos_types::TalosConfig {
    let mut config = talos_types::TalosConfig::default();
    config.llm.provider = provider.to_string();
    config.llm.model = model.to_string();
    let models = match provider {
        "anthropic" => &mut config.llm.anthropic.models,
        "openai" => &mut config.llm.openai.models,
        other => panic!("unsupported test provider {other}"),
    };
    models.push(provider_model(model));
    config
}

fn source() -> SessionSource {
    SessionSource {
        source_type: SessionSourceType::System,
        source_provider: SessionSourceProvider::Http,
        channel_id: None,
        thread_id: None,
        environment: None,
    }
}

fn create_session_params(
    source: SessionSource,
    target_agent: &str,
    model_selection: Option<ModelSelection>,
) -> CreateSessionParams {
    CreateSessionParams {
        session_id: None,
        source,
        user_id: "user-1".to_string(),
        target_agent: target_agent.to_string(),
        execution_node_id: "embedded".to_string(),
        model_selection,
        conf: None,
        workspace_id: "default".to_string(),
        project_id: None,
        current_directory: None,
    }
}

#[tokio::test]
async fn agent_context_is_built_once_from_session_authorities() {
    let config = config_with_model("anthropic", "global-model");
    let expected_workspace = config.resolve_workspace("default", "user-1").unwrap();
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new().with_project(project_record("project-1", "user-1"))),
        config,
    );
    let mut session = service
        .create_session(create_session_params(source(), "agent-1", None))
        .await
        .unwrap();
    session.project_id = Some("project-1".to_string());
    session.environment = Some(talos_types::talos::TalosSessionEnvironment {
        directory: std::path::PathBuf::from("/remote/workspace"),
        connection_mode: talos_types::talos::TalosConnectionMode::Remote,
    });
    let identity = AgentTurnRef::new(session.id.clone(), "turn-1");
    let model_selection = session.conf.model_selection.clone().unwrap();
    let expected = AgentContext {
        history_cursor: None,
        session: AgentSessionContext {
            agent_id: "agent-1".to_string(),
            session_id: identity.session_id.clone(),
            workspace: expected_workspace.clone(),
        },
        turn_id: identity.turn_id.clone(),
        project_id: Some("project-1".to_string()),
        working_directory: Some(std::path::PathBuf::from("/tmp/project")),
        environment: session.environment.clone(),
        model: talos_types::ModelInfo::from_provider_model(
            "anthropic".to_string(),
            provider_model("global-model"),
        ),
        model_selection,
        permission_mode: UserPermissionMode::FullAccess,
    };

    assert_eq!(
        service
            .build_agent_context(&session, identity)
            .await
            .unwrap(),
        expected
    );
    assert_eq!(
        service.build_agent_session_context(&session).await.unwrap(),
        AgentSessionContext {
            agent_id: session.agent_id.clone(),
            session_id: session.id.clone(),
            workspace: expected_workspace,
        }
    );
}

#[tokio::test]
async fn create_session_without_selection_uses_global_default() {
    let base_config = config_with_model("anthropic", "global-model");
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .expect("session should be created");

    assert_eq!(
        session.conf.model_selection,
        Some(ModelSelection::without_reasoning(ModelRef {
            provider: "anthropic".to_string(),
            id: "global-model".to_string(),
        }))
    );
}

#[tokio::test]
async fn create_session_persists_configured_default_reasoning_effort() {
    let mut config = config_with_model("anthropic", "reasoning-model");
    config.llm.anthropic.models[0].reasoning = true;
    config.llm.anthropic.models[0].reasoning_effort = talos_config::ReasoningEffort::High;
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();

    assert_eq!(
        session.conf.model_selection.unwrap().reasoning_effort,
        Some(talos_config::ReasoningEffort::High)
    );
}

#[tokio::test]
async fn create_session_resolves_configured_permission_mode() {
    let mut config = config_with_model("anthropic", "default-model");
    config
        .agent
        .kernel
        .tools
        .sandbox_policy
        .user_permission_mode = talos_config::ToolUserPermissionModeConfig::ApproveForMe;
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .expect("session should be created");
    let info = service.session_info(&session).await.unwrap();

    assert_eq!(
        (info.configured_default_mode, info.effective_mode,),
        (
            UserPermissionMode::ApproveForMe,
            UserPermissionMode::ApproveForMe,
        )
    );
}

#[tokio::test]
async fn create_session_permission_override_wins_over_configured_default() {
    let mut config = config_with_model("anthropic", "default-model");
    config
        .agent
        .kernel
        .tools
        .sandbox_policy
        .user_permission_mode = talos_config::ToolUserPermissionModeConfig::ApproveForMe;
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );
    let mut params = create_session_params(source(), "default", None);
    params.conf = Some(SessionConf {
        model_selection: None,
        user_permission_mode: Some(UserPermissionMode::AskForApproval),
    });

    let session = service
        .create_session(params)
        .await
        .expect("session should be created");
    let info = service.session_info(&session).await.unwrap();

    assert_eq!(
        (
            session.conf.user_permission_mode,
            info.configured_default_mode,
            info.effective_mode,
        ),
        (
            Some(UserPermissionMode::AskForApproval),
            UserPermissionMode::ApproveForMe,
            UserPermissionMode::AskForApproval,
        )
    );
}

#[tokio::test]
async fn runtime_config_reload_changes_new_session_default_model() {
    let base_config = config_with_model("anthropic", "old-model");
    let worker_config = Arc::new(std::sync::RwLock::new(base_config.clone()));
    let coordinator = crate::test_support::with_worker_environment(
        Arc::new(crate::test_support::NoopAgentCoordinator),
        Arc::new(crate::WorkerEnvironmentService::new(
            worker_config.clone(),
            Some(std::env::temp_dir()),
        )),
    );
    let service = super::SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
        coordinator,
    );
    let llm = talos_config::LlmConfig {
        provider: "openai".to_string(),
        model: "new-model".to_string(),
        openai: talos_config::ProviderConfig {
            models: vec![provider_model("new-model")],
            ..Default::default()
        },
        ..Default::default()
    };

    let mut reloaded_config = config_with_model("openai", "new-model");
    reloaded_config.llm = llm;
    *worker_config.write().unwrap() = reloaded_config.clone();
    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .expect("session should be created");

    assert_eq!(
        session.conf.model_selection.unwrap().model_ref,
        ModelRef {
            provider: "openai".to_string(),
            id: "new-model".to_string(),
        }
    );
}

#[tokio::test]
async fn requested_session_model_wins_when_provider_is_unchanged() {
    let mut base_config = talos_types::TalosConfig::default();
    base_config.llm.provider = "anthropic".to_string();
    base_config.llm.model = "old-default".to_string();
    base_config.llm.anthropic.models = vec![provider_model("old-default")];
    let worker_config = Arc::new(std::sync::RwLock::new(base_config.clone()));
    let coordinator = crate::test_support::with_worker_environment(
        Arc::new(crate::test_support::NoopAgentCoordinator),
        Arc::new(crate::WorkerEnvironmentService::new(
            worker_config.clone(),
            Some(std::env::temp_dir()),
        )),
    );
    let service = super::SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
        coordinator,
    );
    let mut llm = talos_config::LlmConfig {
        provider: "anthropic".to_string(),
        model: "new-default".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![
        provider_model("new-default"),
        provider_model("session-model"),
    ];

    let mut reloaded_config = talos_types::TalosConfig::default();
    reloaded_config.llm = llm;
    *worker_config.write().unwrap() = reloaded_config.clone();
    let session = service
        .create_session(create_session_params(
            source(),
            "default",
            Some(ModelSelection::without_reasoning(ModelRef {
                provider: "anthropic".to_string(),
                id: "session-model".to_string(),
            })),
        ))
        .await
        .expect("session should be created");

    assert_eq!(
        session.conf.model_selection.unwrap().model_ref,
        ModelRef {
            provider: "anthropic".to_string(),
            id: "session-model".to_string(),
        }
    );
}

#[tokio::test]
async fn create_session_persists_one_model_selection_in_conf() {
    let mut base_config = config_with_model("anthropic", "default-model");
    base_config.llm.openai.models = vec![provider_model("conf-model")];
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let mut params = create_session_params(source(), "default", None);
    params.conf = Some(SessionConf {
        model_selection: Some(ModelSelection::without_reasoning(ModelRef {
            provider: "openai".to_string(),
            id: "conf-model".to_string(),
        })),
        ..SessionConf::default()
    });

    let session = service
        .create_session(params)
        .await
        .expect("session should be created");

    assert_eq!(
        session.conf.model_selection.as_ref().unwrap().model_ref,
        ModelRef {
            provider: "openai".to_string(),
            id: "conf-model".to_string(),
        }
    );
    let records = store.records.lock().unwrap();
    assert_eq!(records[0].conf, session.conf);
}

#[tokio::test]
async fn existing_session_keeps_persisted_selection_when_default_changes() {
    let base_config = config_with_model("anthropic", "old-default");
    let store = Arc::new(RecordingSessionStore::default());
    let mut service =
        crate::test_support::session_service(store, Arc::new(MockProjectStore::new()), base_config);

    let original = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .expect("session should be created");
    assert_eq!(
        original.conf.model_selection.as_ref().unwrap().model_ref.id,
        "old-default"
    );

    let mut llm = talos_config::LlmConfig {
        provider: "anthropic".to_string(),
        model: "new-default".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![provider_model("old-default"), provider_model("new-default")];
    let mut reloaded_config = config_with_model("anthropic", "new-default");
    reloaded_config.llm = llm;
    service.coordinator = crate::test_support::with_environment(
        Arc::new(crate::test_support::NoopAgentCoordinator),
        reloaded_config,
    );

    let reloaded = service
        .load_record(&original.id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");

    assert_eq!(
        reloaded.conf.model_selection.as_ref().unwrap().model_ref.id,
        "old-default"
    );
}

#[tokio::test]
async fn session_without_explicit_model_selection_is_rejected_without_writing() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].conf = SessionConf::default();
    }

    let error = service
        .list_sessions(SessionQuery::default())
        .await
        .expect_err("persisted session without model selection must fail");

    assert!(error
        .to_string()
        .contains("incompatible persisted session format"));
    assert_eq!(
        store.records.lock().unwrap()[0].conf,
        SessionConf::default(),
        "failed reads must not mutate persisted session state"
    );
}

#[tokio::test]
async fn create_session_preserves_required_default_workspace_binding() {
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );

    let session = service
        .create_session(CreateSessionParams {
            workspace_id: "default".to_string(),
            ..create_session_params(source(), "default", None)
        })
        .await
        .expect("session should be created");

    assert_eq!(session.workspace_id, "default".to_string());
}

#[tokio::test]
async fn create_session_uses_a_non_default_configured_workspace_definition() {
    let mut config = config_with_model("anthropic", "claude-3-5-sonnet");
    config.workspaces = talos_config::WorkspaceConfig {
        default_workspace_id: "team".to_string(),
        definitions: vec![talos_config::WorkspaceDefinition::new(
            "team",
            "workspace-team",
        )],
    };
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(CreateSessionParams {
            workspace_id: service
                .worker_environment("embedded", "user-1")
                .await
                .unwrap()
                .workspace_info(super::SessionService::environment_context("user-1", None))
                .await
                .unwrap()
                .workspace_id,
            ..create_session_params(source(), "default", None)
        })
        .await
        .unwrap();

    assert_eq!(session.workspace_id, "team");
}

#[tokio::test]
async fn create_session_rejects_an_empty_workspace_binding() {
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );

    let error = service
        .create_session(CreateSessionParams {
            workspace_id: String::new(),
            ..create_session_params(source(), "default", None)
        })
        .await
        .expect_err("empty workspace binding must be rejected");

    assert!(error.to_string().contains("unsupported workspace_id"));
}

#[tokio::test]
async fn create_session_persists_an_optional_owned_project_binding() {
    let store = Arc::new(RecordingSessionStore::default());
    let projects = MockProjectStore::new().with_project(project_record("project-1", "user-1"));
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(projects),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );
    let mut params = create_session_params(source(), "default", None);
    params.project_id = Some("project-1".to_string());

    let session = service.create_session(params).await.unwrap();

    assert_eq!(session.project_id.as_deref(), Some("project-1"));
    assert_eq!(
        store.records.lock().unwrap()[0].project_id.as_deref(),
        Some("project-1")
    );
}

#[tokio::test]
async fn create_session_rejects_missing_or_cross_user_project_bindings() {
    let store = Arc::new(RecordingSessionStore::default());
    let projects = MockProjectStore::new().with_project(project_record("project-1", "user-2"));
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(projects),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );

    for project_id in ["missing", "project-1"] {
        let mut params = create_session_params(source(), "default", None);
        params.project_id = Some(project_id.to_string());
        let error = service.create_session(params).await.unwrap_err();
        assert!(error.is_code(talos_error::ErrorCode::ResourceNotFound));
    }
    assert!(store.records.lock().unwrap().is_empty());
}

#[tokio::test]
async fn reset_clears_the_complete_session_history_summary() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].title = "Previous conversation".to_string();
        records[0].message_count = 2;
        records[0].latest_turn_seq = 1;
        records[0].memory_processed_until_event_seq = 3;
        records[0].usage.tokens.total_tokens = 40;
        records[0].usage.context = ContextUsage::new(20, 100, 80, 1);
    }

    let session = service.load_record(&session.id).await.unwrap().unwrap();
    service.clear_session_history(session).await.unwrap();

    let record = store.records.lock().unwrap()[0].clone();
    assert_eq!(
        (
            record.title,
            record.message_count,
            record.latest_turn_seq,
            record.memory_processed_until_event_seq,
            record.usage,
        ),
        (
            talos_types::DEFAULT_SESSION_TITLE.to_string(),
            0,
            0,
            0,
            talos_types::SessionUsage::default(),
        )
    );
}

#[tokio::test]
async fn model_selection_change_preserves_usage() {
    let mut config = config_with_model("anthropic", "model-a");
    config.llm.anthropic.models.push(provider_model("model-b"));
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config,
    );
    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].usage.tokens.total_tokens = 40;
        records[0].usage.context = ContextUsage::new(20, 100, 80, 1);
    }
    let session = service.load_record(&session.id).await.unwrap().unwrap();

    service
        .patch_session(
            session.clone(),
            SessionPatch {
                conf: Some(SessionConf {
                    model_selection: Some(ModelSelection::without_reasoning(ModelRef {
                        provider: "anthropic".to_string(),
                        id: "model-b".to_string(),
                    })),
                    ..SessionConf::default()
                }),
                ..SessionPatch::default()
            },
        )
        .await
        .unwrap();

    let record = store.records.lock().unwrap()[0].clone();
    assert_eq!(record.usage, session.usage);
}

#[tokio::test]
async fn compaction_usage_preserves_the_measured_context_snapshot() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(source(), "default", None))
        .await
        .unwrap();
    let context_usage = ContextUsage::new(20, 100, 80, 1);
    store.records.lock().unwrap()[0].usage.context = context_usage.clone();
    let session = service.load_record(&session.id).await.unwrap().unwrap();

    service
        .record_compaction_usage(
            session,
            &TokenUsage {
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: 5,
                ..TokenUsage::default()
            },
        )
        .await
        .unwrap();

    let record = store.records.lock().unwrap()[0].clone();
    assert_eq!(
        record.usage,
        SessionUsage {
            tokens: TokenUsage {
                input_tokens: 3,
                output_tokens: 2,
                total_tokens: 5,
                ..TokenUsage::default()
            },
            context: context_usage,
        }
    );
}

#[tokio::test]
async fn ordinary_delete_rejects_a_subagent_session() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = crate::test_support::session_service(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let child = service
        .create_subagent_session(
            create_session_params(source(), "default", None),
            talos_types::AgentTurnRef {
                session_id: "root-session".to_string(),
                turn_id: "root-turn".to_string(),
            },
            "root-session".to_string(),
            "/root/reviewer".parse().unwrap(),
        )
        .await
        .unwrap();
    let child_id = child.id.clone();

    assert!(service
        .delete_session(child.clone())
        .await
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(service.delete_subagent_session(child).await.unwrap());
    assert!(store
        .records
        .lock()
        .unwrap()
        .iter()
        .all(|record| record.id != child_id));
}

#[tokio::test]
async fn existing_control_plane_workspace_is_not_an_implicit_working_directory() {
    let directory = tempfile::tempdir().unwrap();
    let config = config_with_model("anthropic", "global-model").with_workspace_definition(
        talos_config::WorkspaceDefinition::new("default", directory.path()),
    );
    let workspace = config.resolve_workspace("default", "user-1").unwrap();
    std::fs::create_dir_all(&workspace.root).unwrap();
    let service = crate::test_support::session_service(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );
    let session = service
        .create_session(create_session_params(source(), "agent-1", None))
        .await
        .unwrap();
    let context = service
        .build_agent_context(&session, AgentTurnRef::new(&session.id, "turn-1"))
        .await
        .unwrap();
    assert_eq!(context.session.workspace, workspace);
    assert_eq!(context.working_directory, None);
}

#[path = "execution_boundary_tests.rs"]
mod execution_boundary_tests;
