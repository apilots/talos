use std::path::PathBuf;

use talos_sdk::{
    AuthSessionReadResponse, AuthSessionSnapshot, AuthSessionUserSnapshot, ModelRef,
    ModelSelection, ReasoningEffort, SessionInfo, UserRole,
};
use talos_types::talos::{StopReason, TokenUsage, TurnExecutionMetrics};

use super::{render_status_card, TuiStatusSnapshot};
use crate::test_support::{mock_model_info, set_test_model_info};
use crate::TuiApp;

fn session_info() -> SessionInfo {
    SessionInfo {
        id: "sess-status".to_string(),
        title: "Status Session".to_string(),
        user_id: "status-user".to_string(),
        source_type: talos_types::talos::TalosSourceType::Cli,
        provider: talos_types::talos::TalosProvider::Cli,
        model_selection: ModelSelection {
            model_ref: ModelRef {
                provider: "mock".to_string(),
                id: "qwen-plus".to_string(),
            },
            reasoning_effort: None,
        },
        configured_default_mode: talos_types::UserPermissionMode::FullAccess,
        effective_mode: talos_types::UserPermissionMode::FullAccess,
        created_at: 1,
        last_activity: 2,
        message_count: 3,
        usage: talos_sdk::SessionUsage {
            tokens: TokenUsage {
                input_tokens: 120_000,
                output_tokens: 40_000,
                total_tokens: 160_000,
                cache_creation_input_tokens: Some(3_000),
                cache_read_input_tokens: Some(21_000),
            },
            context: talos_sdk::ContextUsage::new(160_000, 200_000, 180_000, 1),
        },
        project_id: None,
        current_directory: Some(PathBuf::from("/tmp/talos-project")),
    }
}

fn auth_session() -> AuthSessionReadResponse {
    AuthSessionReadResponse {
        session: Some(AuthSessionSnapshot {
            schema_version: 1,
            cloud_api_base_url: "https://cloud.example.test".to_string(),
            access_token: "test-token".to_string(),
            token_type: "Bearer".to_string(),
            expires_at_ms: None,
            last_used_at_ms: "1".to_string(),
            user: AuthSessionUserSnapshot {
                user_id: "status-user".to_string(),
                username: "status".to_string(),
                email: Some("status@example.com".to_string()),
                role: UserRole::User,
                r#type: Default::default(),
            },
        }),
    }
}

#[test]
fn test_status_snapshot_renders_card_from_real_state() {
    let mut app = TuiApp::build(None);
    set_test_model_info(&mut app, "qwen-plus", 200_000);
    app.available_model_info[0].provider = "bailian".to_string();
    app.available_model_info[0].model.reasoning = true;
    app.available_model_info[0].model.reasoning_effort = ReasoningEffort::High;
    app.current_model = "qwen-plus".to_string();
    app.current_model_provider = "bailian".to_string();
    app.reasoning_effort_override = Some(ReasoningEffort::High);
    app.connection_status = "connected".to_string();
    app.current_project_root = Some(PathBuf::from("/tmp/talos-workspace"));
    app.trace_event_count = 128;
    app.turn_metrics = Some(TurnExecutionMetrics {
        model_rounds: 4,
        tool_calls: 7,
        tool_successes: 6,
        tool_errors: 1,
        stop_reason: StopReason::EndTurn,
        usage: TokenUsage::default(),
        duration_ms: Some(9_600),
    });

    let mut session = session_info();
    session.model_selection.model_ref.provider = "bailian".to_string();
    session.model_selection.reasoning_effort = Some(ReasoningEffort::Light);
    let snapshot = TuiStatusSnapshot::from_app(&app, Some(&session), &auth_session()).unwrap();
    let card = render_status_card(&snapshot);

    assert!(!card.lines().any(|line| line == "/status"));
    assert!(card.contains("Talos TUI"));
    assert!(card.contains("Model:            qwen-plus (bailian, reasoning light, summaries auto)"));
    assert!(card.contains("Directory:        /tmp/talos-project"));
    assert!(card.contains("Account:          status@example.com"));
    assert!(card.contains("Session:          sess-status"));
    assert!(card.contains("Runtime:          embedded ● connected"));
    assert!(card.contains("Permissions:      Full access"));
    assert!(card.contains("[■■■■■■■■■■■■■■■■□□□□] 80.0% used"));
    assert!(card.contains("160.0K used / 200.0K total · 40.0K left"));
    assert!(card.contains("160.0K tokens · cache read 21.0K"));
    assert!(card.contains("last · 4 rounds · 7 tools · 6 ok · 1 err · 9.6s"));
    assert!(card.contains("Events:           128"));
}

#[test]
fn test_status_snapshot_rejects_missing_model_metadata() {
    let mut app = TuiApp::build(None);
    app.available_model_info = vec![mock_model_info("other-model", 128_000)];
    app.current_model = "qwen-plus".to_string();

    let err = TuiStatusSnapshot::from_app(&app, None, &auth_session()).unwrap_err();

    assert!(err
        .to_string()
        .contains("status model metadata missing for qwen-plus"));
}
