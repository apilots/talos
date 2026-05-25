use talos_application_service::Actor;
use talos_auth::RequestIdentity;

pub(super) fn shared_actor(actor: &RequestIdentity) -> Actor {
    if actor.has_full_access() {
        Actor::full(actor.user_id.clone())
    } else {
        Actor::regular(actor.user_id.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use talos_auth::UserRole;
    use talos_types::talos::{TalosContext, TalosProvider, TalosSourceType, TalosUsage};
    use talos_types::{
        ProjectPatch, ProjectRecord, SessionPatch, SessionPatchAck, SessionQuery, SessionService,
        TalosConfig, TalosSession,
    };

    use talos_application_service::ResourceAccessService;
    use talos_auth::RequestIdentity;
    use talos_error::TalosResult;

    use super::shared_actor;

    #[derive(Default)]
    struct RecordingSessionService {
        created_contexts: Mutex<Vec<TalosContext>>,
        listed_queries: Mutex<Vec<SessionQuery>>,
        counted_queries: Mutex<Vec<SessionQuery>>,
        deleted_sessions: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SessionService for RecordingSessionService {
        async fn create_session(
            &self,
            context: TalosContext,
            _model: Option<String>,
            conf: Option<talos_types::SessionConf>,
            project_id: Option<String>,
            current_directory: Option<PathBuf>,
        ) -> TalosResult<TalosSession> {
            self.created_contexts.lock().unwrap().push(context.clone());
            Ok(TalosSession {
                title: "Untitled Session".to_string(),
                user_id: context.user_id.clone(),
                context: talos_types::SessionContext {
                    session_id: "session-1".to_string(),
                    workspace_id: None,
                    source_type: context.source_type,
                    provider: context.provider,
                    user_id: context.user_id,
                    message_id: context.message_id,
                    trace_id: context.trace_id,
                    channel_id: context.channel_id,
                    thread_id: context.thread_id,
                    project_id: project_id.clone(),
                    current_directory: current_directory.clone(),
                    project_root: context.project_root,
                    environment: context.environment,
                },
                conf: conf.unwrap_or_default(),
                project_id,
                current_directory,
                config: TalosConfig::default(),
                state: talos_types::SessionState::default(),
            })
        }

        async fn list_sessions(&self, query: SessionQuery) -> TalosResult<Vec<TalosSession>> {
            self.listed_queries.lock().unwrap().push(query.clone());
            Ok(vec![TalosSession {
                title: "Untitled Session".to_string(),
                user_id: query
                    .user_id
                    .clone()
                    .unwrap_or_else(|| "external-user".to_string()),
                context: talos_types::SessionContext {
                    session_id: "session-1".to_string(),
                    workspace_id: None,
                    source_type: TalosSourceType::Http,
                    provider: TalosProvider::Http,
                    user_id: query.user_id.unwrap_or_else(|| "external-user".to_string()),
                    message_id: "message-1".to_string(),
                    trace_id: "trace-1".to_string(),
                    channel_id: Some("web".to_string()),
                    thread_id: None,
                    project_id: None,
                    current_directory: None,
                    project_root: None,
                    environment: None,
                },
                conf: talos_types::SessionConf::default(),
                project_id: None,
                current_directory: None,
                config: TalosConfig::default(),
                state: talos_types::SessionState {
                    created_at: 0,
                    last_activity: 0,
                    message_count: 0,
                    usage: TalosUsage::default(),
                },
            }])
        }

        async fn count_sessions(&self, query: SessionQuery) -> TalosResult<usize> {
            self.counted_queries.lock().unwrap().push(query);
            Ok(42)
        }

        async fn get_session(&self, _session_id: &str) -> TalosResult<Option<TalosSession>> {
            Ok(None)
        }

        async fn patch_session(
            &self,
            _session_id: &str,
            _patch: SessionPatch,
        ) -> TalosResult<SessionPatchAck> {
            Ok(SessionPatchAck { success: true })
        }

        async fn compact_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in tests")
        }

        async fn clear_session(&self, _session_id: &str) -> TalosResult<TalosSession> {
            unreachable!("not used in tests")
        }

        async fn close_session(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn delete_session(&self, session_id: &str) -> TalosResult<SessionPatchAck> {
            self.deleted_sessions
                .lock()
                .unwrap()
                .push(session_id.to_string());
            Ok(SessionPatchAck { success: true })
        }

        async fn cancel_active_turn(&self, _session_id: &str) -> TalosResult<()> {
            Ok(())
        }

        async fn submit_dynamic_tool_result(
            &self,
            _session_id: &str,
            _call_id: &str,
            _tool_name: &str,
            _output: String,
            _success: bool,
        ) -> TalosResult<()> {
            Ok(())
        }

        async fn list_projects(&self) -> TalosResult<Vec<ProjectRecord>> {
            Ok(vec![
                ProjectRecord {
                    project_id: "project-1".to_string(),
                    name: "Project 1".to_string(),
                    user_id: "user-1".to_string(),
                    directory: PathBuf::from("/tmp/project-1"),
                    created_at: 0,
                    updated_at: 0,
                    session_count: 0,
                },
                ProjectRecord {
                    project_id: "project-2".to_string(),
                    name: "Project 2".to_string(),
                    user_id: "user-2".to_string(),
                    directory: PathBuf::from("/tmp/project-2"),
                    created_at: 0,
                    updated_at: 0,
                    session_count: 0,
                },
            ])
        }

        async fn get_project(&self, _project_id: &str) -> TalosResult<Option<ProjectRecord>> {
            Ok(None)
        }

        async fn create_project(
            &self,
            _name: String,
            _directory: PathBuf,
            _user_id: String,
        ) -> TalosResult<ProjectRecord> {
            unreachable!("not used in tests")
        }

        async fn patch_project(
            &self,
            _project_id: &str,
            _patch: ProjectPatch,
        ) -> TalosResult<Option<ProjectRecord>> {
            Ok(None)
        }

        async fn delete_project(&self, _project_id: &str) -> TalosResult<()> {
            Ok(())
        }
    }

    fn test_context(user_id: &str) -> TalosContext {
        TalosContext::new(
            TalosSourceType::Http,
            TalosProvider::Http,
            user_id,
            "msg-1",
            "trace-1",
        )
        .with_channel_id("web")
    }

    fn member_identity() -> RequestIdentity {
        RequestIdentity {
            user_id: "member-user".to_string(),
            username: Some("member".to_string()),
            role: UserRole::User,
            scopes: Vec::new(),
            auth_disabled: false,
            access_token: None,
        }
    }

    fn admin_identity() -> RequestIdentity {
        RequestIdentity::mock_admin("admin-user")
    }

    fn open_identity() -> RequestIdentity {
        RequestIdentity::auth_disabled()
    }

    #[tokio::test]
    async fn create_session_rewrites_context_for_member_actor() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let session = access
            .create_session(
                &shared_actor(&member_identity()),
                test_context("external-user"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create_session should succeed");

        assert_eq!(session.context.user_id, "member-user");
        assert_eq!(
            service.created_contexts.lock().unwrap()[0].user_id,
            "member-user"
        );
    }

    #[tokio::test]
    async fn create_session_rewrites_context_for_admin_actor_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let session = access
            .create_session(
                &shared_actor(&admin_identity()),
                test_context("external-user"),
                None,
                None,
                None,
                None,
            )
            .await
            .expect("create_session should succeed");

        assert_eq!(session.context.user_id, "admin-user");
        assert_eq!(
            service.created_contexts.lock().unwrap()[0].user_id,
            "admin-user"
        );
    }

    #[tokio::test]
    async fn list_sessions_scopes_owner_for_member_actor() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_sessions(
                &shared_actor(&member_identity()),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("member-user")
        );
    }

    #[tokio::test]
    async fn list_sessions_scopes_owner_for_admin_actor_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_sessions(
                &shared_actor(&admin_identity()),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("admin-user")
        );
    }

    #[tokio::test]
    async fn list_sessions_scopes_owner_for_open_actor_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_sessions(
                &shared_actor(&open_identity()),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: None,
                    offset: None,
                },
            )
            .await
            .expect("list_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("anonymous")
        );
    }

    #[tokio::test]
    async fn list_projects_scopes_admin_to_owned_resources_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_projects(&shared_actor(&admin_identity()))
            .await
            .expect("list_projects should succeed");

        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn list_projects_scopes_open_actor_to_anonymous_resources_on_regular_routes() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_projects(&shared_actor(&open_identity()))
            .await
            .expect("list_projects should succeed");

        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn list_admin_sessions_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let _ = access
            .list_admin_sessions(
                &shared_actor(&admin_identity()),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: Some(20),
                    offset: Some(40),
                },
            )
            .await
            .expect("list_admin_sessions should succeed");

        assert_eq!(
            service.listed_queries.lock().unwrap()[0].user_id.as_deref(),
            Some("external-user")
        );
    }

    #[tokio::test]
    async fn count_admin_sessions_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let total = access
            .count_admin_sessions(
                &shared_actor(&admin_identity()),
                SessionQuery {
                    user_id: Some("external-user".to_string()),
                    channel_id: Some("web".to_string()),
                    provider: Some("http".to_string()),
                    project_id: None,
                    limit: Some(20),
                    offset: Some(40),
                },
            )
            .await
            .expect("count_admin_sessions should succeed");

        assert_eq!(total, 42);
        assert_eq!(
            service.counted_queries.lock().unwrap()[0]
                .user_id
                .as_deref(),
            Some("external-user")
        );
    }

    #[tokio::test]
    async fn list_admin_projects_keeps_admin_visibility() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service);

        let projects = access
            .list_admin_projects(&shared_actor(&admin_identity()))
            .await
            .expect("list_admin_projects should succeed");

        assert_eq!(projects.len(), 2);
    }

    #[tokio::test]
    async fn delete_admin_session_bypasses_regular_visibility_scoping() {
        let service = Arc::new(RecordingSessionService::default());
        let access = ResourceAccessService::new(service.clone());

        let ack = access
            .delete_admin_session(&shared_actor(&admin_identity()), "external-session")
            .await
            .expect("delete_admin_session should succeed");

        assert!(ack.success);
        assert_eq!(
            service.deleted_sessions.lock().unwrap().as_slice(),
            &["external-session".to_string()]
        );
    }
}
