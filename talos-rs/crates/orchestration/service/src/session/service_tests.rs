use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use talos_error::{TalosError, TalosResult};
use talos_store::project::MockProjectStore;
use talos_store::session::{
    MockSessionStore, SessionQuery, SessionRecord, SessionStore, SessionSummary,
    SessionTreeIdentity,
};
use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TokenUsage};
use talos_types::{
    ContextUsage, ModelRef, ModelSelection, SessionConf, SessionInfo, SessionPatch,
    UserPermissionMode,
};

use super::{CreateSessionParams, SessionService};

#[derive(Default)]
struct RecordingSessionStore {
    records: Mutex<Vec<SessionRecord>>,
    failing_updates: AtomicUsize,
}

#[async_trait]
impl SessionStore for RecordingSessionStore {
    async fn insert_session_record(&self, record: &SessionRecord, _scope: &str) -> TalosResult<()> {
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

    async fn update_session_record(&self, record: &SessionRecord) -> TalosResult<bool> {
        if self
            .failing_updates
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            return Err(TalosError::internal("injected update failure".to_string()));
        }
        let mut records = self.records.lock().unwrap();
        let Some(stored) = records.iter_mut().find(|stored| stored.id == record.id) else {
            return Ok(false);
        };
        *stored = record.clone();
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

    async fn advance_memory_processed_until_seq(
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
            record.memory_processed_until_seq = next_until_seq;
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

fn context() -> TalosContext {
    TalosContext {
        source_type: TalosSourceType::System,
        provider: TalosProvider::Http,
        user_id: "user-1".to_string(),
        message_id: "message-1".to_string(),
        trace_id: "trace-1".to_string(),
        channel_id: None,
        thread_id: None,
        project_root: None,
        environment: None,
    }
}

fn create_session_params(
    context: TalosContext,
    target_agent: &str,
    model_selection: Option<ModelSelection>,
) -> CreateSessionParams {
    CreateSessionParams {
        session_id: None,
        context,
        target_agent: target_agent.to_string(),
        model_selection,
        conf: None,
        workspace_id: None,
        project_id: None,
        current_directory: None,
    }
}

#[tokio::test]
async fn create_session_without_selection_uses_global_default() {
    let base_config = config_with_model("anthropic", "global-model");
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "global-model");
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
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(create_session_params(context(), "default", None))
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
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );

    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");
    let info = SessionInfo::from(&session);

    assert_eq!(
        (
            session.effective_user_permission_mode,
            info.configured_default_mode,
            info.effective_mode,
        ),
        (
            UserPermissionMode::ApproveForMe,
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
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config,
    );
    let mut params = create_session_params(context(), "default", None);
    params.conf = Some(SessionConf {
        model_selection: None,
        user_permission_mode: Some(UserPermissionMode::AskForApproval),
    });

    let session = service
        .create_session(params)
        .await
        .expect("session should be created");
    let info = SessionInfo::from(&session);

    assert_eq!(
        (
            session.conf.user_permission_mode,
            session.effective_user_permission_mode,
            info.configured_default_mode,
            info.effective_mode,
        ),
        (
            Some(UserPermissionMode::AskForApproval),
            UserPermissionMode::AskForApproval,
            UserPermissionMode::ApproveForMe,
            UserPermissionMode::AskForApproval,
        )
    );
}

#[tokio::test]
async fn update_base_llm_changes_new_session_default_model() {
    let base_config = config_with_model("anthropic", "old-model");
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
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

    service.update_base_llm(&llm);
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "openai");
    assert_eq!(session.config.llm.model, "new-model");
}

#[tokio::test]
async fn requested_session_model_wins_when_provider_is_unchanged() {
    let mut base_config = talos_types::TalosConfig::default();
    base_config.llm.provider = "anthropic".to_string();
    base_config.llm.model = "old-default".to_string();
    base_config.llm.anthropic.models = vec![provider_model("old-default")];
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        base_config,
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

    service.update_base_llm(&llm);
    let session = service
        .create_session(create_session_params(
            context(),
            "default",
            Some(ModelSelection::without_reasoning(ModelRef {
                provider: "anthropic".to_string(),
                id: "session-model".to_string(),
            })),
        ))
        .await
        .expect("session should be created");

    assert_eq!(session.config.llm.provider, "anthropic");
    assert_eq!(session.config.llm.model, "session-model");
}

#[tokio::test]
async fn create_session_conf_model_selection_matches_stored_model() {
    let mut base_config = config_with_model("anthropic", "default-model");
    base_config.llm.openai.models = vec![provider_model("conf-model")];
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        base_config,
    );
    let mut params = create_session_params(context(), "default", None);
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

    assert_eq!(session.config.llm.provider, "openai");
    assert_eq!(session.config.llm.model, "conf-model");
    let records = store.records.lock().unwrap();
    assert_eq!(records[0].model, "conf-model");
}

#[tokio::test]
async fn existing_session_keeps_persisted_selection_when_default_changes() {
    let base_config = config_with_model("anthropic", "old-default");
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(store, Arc::new(MockProjectStore::new()), base_config);

    let original = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("session should be created");
    assert_eq!(original.config.llm.provider, "anthropic");
    assert_eq!(original.config.llm.model, "old-default");

    let mut llm = talos_config::LlmConfig {
        provider: "anthropic".to_string(),
        model: "new-default".to_string(),
        ..Default::default()
    };
    llm.anthropic.models = vec![provider_model("old-default"), provider_model("new-default")];
    service.update_base_llm(&llm);

    let reloaded = service
        .get_session(&original.context.session_id)
        .await
        .expect("session read should succeed")
        .expect("session should exist");

    assert_eq!(reloaded.config.llm.provider, "anthropic");
    assert_eq!(reloaded.config.llm.model, "old-default");
}

#[tokio::test]
async fn session_without_explicit_model_selection_is_rejected_without_writing() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let created = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].conf = SessionConf::default();
    }

    let error = service
        .get_session(&created.context.session_id)
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
async fn create_session_preserves_workspace_routing_key() {
    let service = SessionService::new(
        Arc::new(MockSessionStore::new()),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "claude-3-5-sonnet"),
    );

    let session = service
        .create_session(CreateSessionParams {
            workspace_id: Some("workspace-alpha".to_string()),
            ..create_session_params(context(), "default", None)
        })
        .await
        .expect("session should be created");

    assert_eq!(
        session.context.workspace_id,
        Some("workspace-alpha".to_string())
    );
}

#[tokio::test]
async fn reset_keeps_monotonic_turn_seq_for_the_next_completed_turn() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("create session");
    let session_id = &session.context.session_id;
    service
        .record_persisted_turn_progress(session_id, 1, "current-model", &TokenUsage::default())
        .await
        .expect("record first turn");
    service
        .reset_session(session_id)
        .await
        .expect("reset session");
    service
        .record_persisted_turn_progress(session_id, 2, "current-model", &TokenUsage::default())
        .await
        .expect("record turn after reset");

    let record = store
        .get_session_record(session_id)
        .await
        .expect("read session")
        .expect("session exists");
    assert_eq!((record.message_count, record.latest_turn_seq), (2, 2));
}

#[tokio::test]
async fn reset_clears_session_tokens_and_context_usage() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].usage.tokens.total_tokens = 40;
        records[0].usage.context = ContextUsage::new(20, 100, 80, 1);
    }

    service
        .reset_session(&session.context.session_id)
        .await
        .unwrap();

    let record = store.records.lock().unwrap()[0].clone();
    assert_eq!(record.usage, talos_types::SessionUsage::default());
}

#[tokio::test]
async fn model_selection_change_clears_context_but_preserves_token_totals() {
    let mut config = config_with_model("anthropic", "model-a");
    config.llm.anthropic.models.push(provider_model("model-b"));
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(store.clone(), Arc::new(MockProjectStore::new()), config);
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();
    {
        let mut records = store.records.lock().unwrap();
        records[0].usage.tokens.total_tokens = 40;
        records[0].usage.context = ContextUsage::new(20, 100, 80, 1);
    }

    service
        .patch_session(
            &session.context.session_id,
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
    assert_eq!(record.usage.tokens.total_tokens, 40);
    assert_eq!(record.usage.context, None);
}

#[tokio::test]
async fn record_usage_invalidates_context_snapshot() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .unwrap();
    store.records.lock().unwrap()[0].usage.context = ContextUsage::new(20, 100, 80, 1);

    service
        .record_usage(
            &session.context.session_id,
            "current-model",
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
    assert_eq!(record.usage.tokens.total_tokens, 5);
    assert_eq!(record.usage.context, None);
}

#[tokio::test]
async fn ordinary_delete_rejects_a_subagent_session() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let child = service
        .create_subagent_session(
            create_session_params(context(), "default", None),
            talos_types::AgentTurnRef {
                session_id: "root-session".to_string(),
                turn_id: "root-turn".to_string(),
            },
            "root-session".to_string(),
            "/root/reviewer".parse().unwrap(),
        )
        .await
        .unwrap();
    let child_id = &child.context.session_id;

    assert!(service
        .delete_session(child_id)
        .await
        .is_err_and(|error| error.is_code(talos_error::ErrorCode::InvalidRequest)));
    assert!(
        service
            .delete_subagent_session(child_id)
            .await
            .unwrap()
            .success
    );
}

#[tokio::test]
async fn persisted_turn_progress_recovers_after_a_failed_session_update() {
    let store = Arc::new(RecordingSessionStore::default());
    let service = SessionService::new(
        store.clone(),
        Arc::new(MockProjectStore::new()),
        config_with_model("anthropic", "current-model"),
    );
    let session = service
        .create_session(create_session_params(context(), "default", None))
        .await
        .expect("create session");
    let session_id = &session.context.session_id;
    store.failing_updates.store(1, Ordering::SeqCst);

    service
        .record_persisted_turn_progress(session_id, 1, "current-model", &TokenUsage::default())
        .await
        .expect_err("first progress update should fail");
    service
        .record_persisted_turn_progress(session_id, 2, "current-model", &TokenUsage::default())
        .await
        .expect("next persisted sequence should recover progress");

    let record = store
        .get_session_record(session_id)
        .await
        .expect("read session")
        .expect("session exists");
    assert_eq!((record.message_count, record.latest_turn_seq), (4, 2));
}
