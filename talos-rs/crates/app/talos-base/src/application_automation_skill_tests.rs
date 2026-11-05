//! Internal resolver/ownership coverage. Real direct/outbound RPC is tested separately.
use super::*;
use crate::application_automation_control::ApplicationAutomationSkillResolver;
use agent_protocol::{
    NodeHealth, NodeMode, NodeStatus, WorkerEnvironment, WorkerEnvironmentContext,
};
use talos_config::Config;
use talos_service::{automation::goal_loop::AutomationSkillResolver, PluginManagerService};
use talos_store::node::{
    CreateNodeParams, HeartbeatNodeParams, LeaseCheck, RegisterNodeParams, SetNodeStatusParams,
};
use talos_types::{AutomationMode, SkillScope, SkillSelector, SkillsListRequest};

struct ResolverFixture {
    _home: tempfile::TempDir,
    nodes: Arc<DatabaseAgentNodeStore>,
    sessions: Arc<DatabaseSessionStore>,
    coordinator: Arc<CoordinatorImpl>,
    environment: Arc<talos_service::WorkerEnvironmentService>,
    resolver: ApplicationAutomationSkillResolver,
    session: SessionRecord,
}

impl ResolverFixture {
    async fn new(publish_remote: bool) -> Self {
        let storage = Arc::new(
            SqliteStorage::new_with_max_connections(":memory:", 1)
                .await
                .unwrap(),
        );
        talos_store::migration::run_migrations(
            storage.pool(),
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../conf/migrations/sqlite"),
        )
        .await
        .unwrap();
        let nodes = Arc::new(DatabaseAgentNodeStore::new(storage.pool().clone()));
        let sessions = Arc::new(DatabaseSessionStore::new(storage).await.unwrap());
        let now = chrono::Utc::now().timestamp();
        nodes
            .create(CreateNodeParams {
                node_id: "laptop-1".into(),
                owner_user_id: Some("user-1".into()),
                display_name: "Owned laptop".into(),
                mode: NodeMode::Remote,
                endpoint: None,
                credential_hash: Some([11; 32]),
                credential_expires_at: Some(now + 3600),
                now_at: now,
            })
            .await
            .unwrap();
        nodes
            .register(RegisterNodeParams {
                node_id: "laptop-1".into(),
                expected_credential_hash: [11; 32],
                lease_token_hash: [22; 32],
                lease_expires_at: now + 3600,
                now_at: now,
                system_info: None,
            })
            .await
            .unwrap();
        nodes
            .heartbeat(HeartbeatNodeParams {
                lease: LeaseCheck {
                    node_id: "laptop-1".into(),
                    token_hash: [22; 32],
                    now_at: now,
                },
                health: NodeHealth::Healthy,
            })
            .await
            .unwrap();
        nodes
            .upsert_embedded(NodeStatus::Enabled, now)
            .await
            .unwrap();

        let home = tempfile::tempdir().unwrap();
        let mut config = Config::default()
            .with_runtime_paths(talos_config::TalosPaths::from_home(home.path().into()));
        config.gateway.auth = true;
        config.workspaces = talos_config::WorkspaceConfig {
            default_workspace_id: "changed-default".into(),
            definitions: vec![
                talos_config::WorkspaceDefinition::new("saved", "saved-workspace"),
                talos_config::WorkspaceDefinition::new("changed-default", "other-workspace"),
            ],
        };
        // The same name at another scope cannot select the automation document.
        let external = home.path().join("external/goal");
        std::fs::create_dir_all(&external).unwrap();
        std::fs::write(
            external.join("SKILL.md"),
            "---\nname: goal\ndescription: External collision.\n---\nExternal instructions.",
        )
        .unwrap();
        config.agent.kernel.plugins.roots = vec![talos_config::PluginRootConfig {
            kind: talos_config::PluginRootKind::Skill,
            path: home.path().join("external").display().to_string(),
            read_only: true,
        }];
        let plugins =
            PluginManagerService::with_manager(talos_plugin::PluginManager::with_state_store(
                config.agent.kernel.plugins.clone(),
                talos_plugin::PluginStateStore::new(home.path().join("worker-plugin-state")),
            ));
        assert!(config.gateway.auth);
        let environment = Arc::new(
            talos_service::WorkerEnvironmentService::new(
                Arc::new(std::sync::RwLock::new(config)),
                Some(home.path().into()),
            )
            .with_plugin_manager(plugins),
        );
        let coordinator = Arc::new(CoordinatorImpl::new(
            Arc::new(talos_router::RouterImpl::new()),
            sessions.clone(),
            nodes.clone(),
        ));
        // An available local environment is an explicit no-fallback decoy.
        coordinator
            .register_environment("embedded".into(), environment.clone())
            .await;
        if publish_remote {
            coordinator
                .register_environment("laptop-1".into(), environment.clone())
                .await;
        }
        let mut session = session_record();
        session.workspace_id = "saved".into();
        sessions.insert_session_record(&session).await.unwrap();
        let resolver =
            ApplicationAutomationSkillResolver::new(Arc::downgrade(&coordinator), sessions.clone());
        Self {
            _home: home,
            nodes,
            sessions,
            coordinator,
            environment,
            resolver,
            session,
        }
    }

    fn context(&self) -> WorkerEnvironmentContext {
        WorkerEnvironmentContext {
            user_id: self.session.user_id.clone(),
            username: None,
            workspace_id: Some(self.session.workspace_id.clone()),
        }
    }
}

#[tokio::test]
async fn automation_skill_resolver_uses_authenticated_registered_worker_system_documents() {
    let fixture = ResolverFixture::new(true).await;
    let listed = fixture
        .environment
        .list_skills(fixture.context(), SkillsListRequest::default())
        .await
        .unwrap();
    assert!(listed
        .data
        .iter()
        .any(|skill| skill.scope == SkillScope::External && skill.name == "goal"));
    for mode in [AutomationMode::Goal, AutomationMode::Loop] {
        let actual = fixture
            .resolver
            .resolve(&fixture.session, mode)
            .await
            .unwrap();
        let system = listed
            .data
            .iter()
            .find(|skill| {
                skill.scope == SkillScope::System
                    && skill.read_only
                    && skill.enabled
                    && skill.name == mode.skill_name()
            })
            .unwrap();
        assert_eq!(actual, SkillSelector::selection(&system.name, &system.path));
        assert!(actual.path.as_ref().unwrap().is_file());
    }
    // No public TalosService, Actor, access token or synthetic filesystem selector.
    assert_eq!(
        fixture
            .sessions
            .get_session_record(&fixture.session.id)
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        "saved"
    );
}

#[tokio::test]
async fn automation_skill_resolver_preserves_computer_owner_and_disabled_checks() {
    let fixture = ResolverFixture::new(true).await;
    let mut foreign = fixture.session.clone();
    foreign.id = "foreign-session".into();
    foreign.user_id = "user-2".into();
    fixture
        .sessions
        .insert_session_record(&foreign)
        .await
        .unwrap();
    let error = fixture
        .resolver
        .resolve(&foreign, AutomationMode::Goal)
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::PermissionDenied));
    fixture
        .nodes
        .set_status(SetNodeStatusParams {
            node_id: fixture.session.execution_node_id.clone(),
            status: NodeStatus::Disabled,
            now_at: chrono::Utc::now().timestamp(),
        })
        .await
        .unwrap();
    let error = fixture
        .resolver
        .resolve(&fixture.session, AutomationMode::Loop)
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::ServiceUnavailable));
}

#[tokio::test]
async fn automation_skill_resolver_does_not_fallback_to_local_when_worker_environment_is_unpublished(
) {
    let fixture = ResolverFixture::new(false).await;
    assert!(fixture
        .coordinator
        .worker_environment("embedded", "user-1")
        .await
        .is_ok());
    let error = fixture
        .resolver
        .resolve(&fixture.session, AutomationMode::Goal)
        .await
        .unwrap_err();
    assert!(error.is_code(ErrorCode::ServiceUnavailable));
}

#[tokio::test]
async fn automation_skill_resolver_accepts_claimed_original_namespace_and_keeps_pinned_workspace() {
    let fixture = ResolverFixture::new(true).await;
    let mut record = fixture.session.clone();
    record.id = "claimed-session".into();
    record.execution_node_id = "embedded".into();
    record.user_id = "anonymous".into();
    fixture
        .sessions
        .insert_session_record(&record)
        .await
        .unwrap();
    fixture
        .sessions
        .claim_local_history("user-1")
        .await
        .unwrap();
    let claimed = fixture
        .sessions
        .get_session_record(&record.id)
        .await
        .unwrap()
        .unwrap();
    let namespace = fixture
        .sessions
        .get_workspace_user_id(&record.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.user_id, "user-1");
    assert_eq!(namespace, "anonymous");
    assert_eq!(claimed.workspace_id, "saved");
    let environment = fixture
        .coordinator
        .worker_environment(&claimed.execution_node_id, &claimed.user_id)
        .await
        .unwrap();
    let workspace = environment
        .workspace_info(WorkerEnvironmentContext {
            user_id: namespace.clone(),
            username: None,
            workspace_id: Some(claimed.workspace_id.clone()),
        })
        .await
        .unwrap();
    assert_eq!(workspace.workspace_id, "saved");
    assert!(workspace.user_workspace_dir.contains("anonymous"));
    assert!(workspace.user_workspace_dir.contains("saved-workspace"));
    fixture
        .resolver
        .resolve(&claimed, AutomationMode::Loop)
        .await
        .unwrap();
    assert_eq!(
        fixture
            .sessions
            .get_workspace_user_id(&record.id)
            .await
            .unwrap(),
        Some(namespace)
    );
    assert_eq!(
        fixture
            .sessions
            .get_session_record(&record.id)
            .await
            .unwrap()
            .unwrap()
            .workspace_id,
        "saved"
    );
    // Skills are instance-scoped: these assertions preserve real saved facts,
    // not a claim that this internal-port test inspected a serialized RPC context.
}
