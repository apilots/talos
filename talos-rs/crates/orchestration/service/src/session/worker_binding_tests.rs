use super::*;
use crate::test_support::{with_worker_environment, NoopAgentCoordinator};
use talos_config::{TalosPaths, WorkspaceConfig, WorkspaceDefinition};
use talos_storage::SqliteStorage;
use talos_store::{
    project::{MockProjectStore, ProjectRecord},
    session::DatabaseSessionStore,
};
use talos_types::SessionConf;

#[tokio::test]
async fn session_uses_worker_catalog_and_saved_workspace_after_default_changes() {
    let home = tempfile::tempdir().unwrap();
    let mut worker =
        TalosConfig::default().with_runtime_paths(TalosPaths::from_home(home.path().into()));
    worker.workspaces = WorkspaceConfig {
        default_workspace_id: "pc".into(),
        definitions: vec![
            WorkspaceDefinition::new("pc", "pc-files"),
            WorkspaceDefinition::new("other", "other-files"),
        ],
    };
    worker.llm.model = "worker-model".into();
    worker.llm.anthropic.models = vec![talos_config::ProviderModelConfig {
        id: "worker-model".into(),
        name: "Worker model".into(),
        context_window: 32_768,
        reasoning: false,
        reasoning_effort: Default::default(),
        speed: Default::default(),
        input: Vec::new(),
        cost: Default::default(),
    }];
    let config = Arc::new(RwLock::new(worker));
    let coordinator = with_worker_environment(
        Arc::new(NoopAgentCoordinator),
        Arc::new(crate::WorkerEnvironmentService::new(
            config.clone(),
            Some(home.path().into()),
        )),
    );
    let storage = Arc::new(SqliteStorage::new(":memory:").await.unwrap());
    crate::test_support::run_test_migrations(storage.pool()).await;
    let store = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
    let service = SessionService::new(
        store,
        Arc::new(MockProjectStore::new()),
        TalosConfig::default(),
        coordinator,
    );
    let record = service
        .create_session(CreateSessionParams {
            session_id: Some("remote-session".into()),
            source: talos_types::talos::SessionSource::http(),
            user_id: "user-1".into(),
            target_agent: "default".into(),
            execution_node_id: "worker-a".into(),
            model_selection: None,
            conf: None,
            workspace_id: "pc".into(),
            project_id: None,
            current_directory: None,
        })
        .await
        .unwrap();
    assert_eq!(
        record.conf.model_selection.as_ref().unwrap().model_ref.id,
        "worker-model"
    );
    assert_eq!(service.session_info(&record).unwrap().node_id, "worker-a");
    config.write().unwrap().workspaces.default_workspace_id = "other".into();
    let context = service
        .build_agent_context(&record, AgentTurnRef::new(&record.id, "turn-1"))
        .await
        .unwrap();
    assert_eq!(context.session.workspace.workspace_id, "pc");
    assert_eq!(
        context.session.workspace.root,
        home.path().join("pc-files/user-1")
    );
    assert_eq!(context.model.model.id, "worker-model");
    config
        .write()
        .unwrap()
        .workspaces
        .definitions
        .retain(|definition| definition.workspace_id != "pc");
    assert!(service
        .build_agent_session_context(&record)
        .await
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
    assert_eq!(service.session_info(&record).unwrap().id, "remote-session");
}

#[tokio::test]
async fn project_binding_rejects_another_worker_before_session_is_persisted() {
    let project = ProjectRecord {
        execution_node_id: "worker-a".into(),
        project_id: "project-1".into(),
        name: "Project".into(),
        user_id: "user-1".into(),
        directory: "/worker-only".into(),
        status: Default::default(),
        created_at: 1,
        updated_at: 1,
        session_count: 0,
    };
    let service = crate::test_support::session_service(
        Arc::new(talos_store::session::MockSessionStore::new()),
        Arc::new(MockProjectStore::new().with_project(project)),
        TalosConfig::default(),
    );
    let result = service
        .create_session(CreateSessionParams {
            session_id: Some("session-1".into()),
            source: talos_types::talos::SessionSource::http(),
            user_id: "user-1".into(),
            target_agent: "default".into(),
            execution_node_id: "worker-b".into(),
            model_selection: None,
            conf: Some(SessionConf::default()),
            workspace_id: "default".into(),
            project_id: Some("project-1".into()),
            current_directory: None,
        })
        .await;
    assert!(result
        .unwrap_err()
        .is_code(talos_error::ErrorCode::InvalidRequest));
}
