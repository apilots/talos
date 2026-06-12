use super::*;

#[tokio::test]
async fn test_remote_session_defaults_project_root_to_workspace_dir() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(talos_session_store::MockSessionStore::new());
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);
    coordinator
        .register_agent("default", Arc::new(talos_agent::MockAgent::new()))
        .await
        .expect("default agent should register");

    let session = talos_types::SessionService::create_session(
        &coordinator,
        TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        None,
        None,
        None,
        None,
    )
    .await
    .expect("session creation should succeed");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}

#[tokio::test]
async fn test_remote_session_ignores_current_directory_patch_for_root_resolution() {
    let workspace_dir = std::env::temp_dir().join(format!("talos-coordinator-{}", Uuid::new_v4()));
    let custom_dir = std::env::temp_dir().join(format!("talos-project-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
    std::fs::create_dir_all(&custom_dir).expect("custom dir should be created");

    let mut config = TalosConfig::default();
    config.agent.kernel.workspace_dir = workspace_dir.to_string_lossy().to_string();

    let mock_router = Arc::new(MockRouter::new());
    let session_store = Arc::new(TitleTestSessionStore::new(SessionRecord {
        id: "session-1".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "user-1".to_string(),
        context: TalosContext::new(
            TalosSourceType::Cli,
            TalosProvider::Cli,
            "user-1",
            "message-1",
            "trace-1",
        )
        .with_environment(TalosSessionEnvironment {
            directory: std::path::PathBuf::from("/client/path"),
            connection_mode: TalosConnectionMode::Remote,
        }),
        model: config.llm.model.clone(),
        created_at: 0,
        last_activity: 0,
        message_count: 0,
        usage: TalosUsage::default(),
        conf: SessionConf::default(),
        project_id: None,
        current_directory: Some(custom_dir.clone()),
    }));
    let coordinator = CoordinatorImpl::new(mock_router, session_store, config);

    talos_types::SessionService::patch_session(
        &coordinator,
        "session-1",
        SessionPatch {
            conf: None,
            project_id: None,
            current_directory: Some(OptionalCurrentDirectoryPatch::Set {
                path: custom_dir.clone(),
            }),
        },
    )
    .await
    .expect("patch should succeed");

    let session = talos_types::SessionService::get_session(&coordinator, "session-1")
        .await
        .expect("get session should succeed")
        .expect("session should exist");

    assert_eq!(
        session.context.project_root.as_deref(),
        Some(workspace_dir.as_path())
    );
}
