use std::path::Path;

use talos_config::{Config, RuntimeEnv};
use talos_types::talos::{TalosProvider, TalosSourceType};
use talos_types::{SessionConf, SessionContext, SessionState, TalosSession, DEFAULT_SESSION_TITLE};

use super::AgentHarnessImpl;

#[test]
fn runtime_kernel_config_uses_session_runtime_env() {
    let mut session = TalosSession {
        agent_id: "default".to_string(),
        title: DEFAULT_SESSION_TITLE.to_string(),
        user_id: "talos-test-local-user".to_string(),
        context: SessionContext {
            session_id: "runtime-env-session".to_string(),
            workspace_id: "default".to_string(),
            source_type: TalosSourceType::Cli,
            provider: TalosProvider::Cli,
            user_id: "talos-test-local-user".to_string(),
            message_id: "message-1".to_string(),
            trace_id: "trace-1".to_string(),
            channel_id: None,
            thread_id: None,
            project_id: None,
            current_directory: None,
            project_root: None,
            environment: None,
        },
        conf: SessionConf::default(),
        project_id: None,
        current_directory: None,
        config: Config::default(),
        effective_user_permission_mode: talos_types::UserPermissionMode::FullAccess,
        state: SessionState {
            created_at: 1,
            last_activity: 1,
            message_count: 0,
            usage: talos_types::SessionUsage::default(),
        },
    };
    session.config.agent.kernel.runtime_env = RuntimeEnv::load_from_iter(
        Path::new("__missing_session_env__"),
        [("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY", "session-key")],
    )
    .expect("session runtime env should load");

    let kernel_config = AgentHarnessImpl::kernel_config_for_session(&session)
        .expect("kernel config should resolve");

    assert_eq!(
        kernel_config
            .runtime_env
            .get("TALOS_WEB_SEARCH_FIRECRAWL_API_KEY"),
        Some("session-key")
    );
}
