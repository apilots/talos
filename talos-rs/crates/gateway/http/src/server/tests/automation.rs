use super::*;
use crate::server::automation::*;
use talos_service::automation::goal_loop::AutomationSkillResolver;
use talos_types::{AutomationAction, AutomationRequest, GoalAction, LoopAction, LoopWakeDecision};

struct TestAutomationSkills;
impl AutomationSkillResolver for TestAutomationSkills {
    fn resolve<'a>(
        &'a self,
        _session: &'a talos_store::session::SessionRecord,
        mode: talos_types::AutomationMode,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = TalosResult<talos_types::SkillSelector>> + Send + 'a>,
    > {
        Box::pin(async move {
            Ok(talos_types::SkillSelector::selection(
                mode.skill_name(),
                format!("/system/{}/SKILL.md", mode.skill_name()),
            ))
        })
    }
}

#[tokio::test]
async fn automation_http_uses_shared_commands_and_original_error_contract() {
    let state = test_state(false).await;
    let resolved = resolve_automation_command_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(talos_types::AutomationCommandResolveRequest {
            text: "/loop 5m check CI".into(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        expect_success(resolved.0).command,
        talos_types::resolve_automation_command("/loop 5m check CI").unwrap()
    );
    let raw_prompt = resolve_automation_command_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(talos_types::AutomationCommandResolveRequest {
            text: "/loop 0m check CI".into(),
        }),
    )
    .await
    .unwrap();
    assert_eq!(
        expect_success(raw_prompt.0).command,
        Some(talos_types::AutomationCommand::Activate {
            mode: talos_types::AutomationMode::Loop,
            prompt: "0m check CI".into(),
        })
    );
    let invalid = resolve_automation_command_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(talos_types::AutomationCommandResolveRequest {
            text: "/loop stop".into(),
        }),
    )
    .await
    .unwrap_err()
    .into_response();
    let body = axum::body::to_bytes(invalid.into_body(), usize::MAX)
        .await
        .unwrap();
    let value: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["status"], "error");
    assert_eq!(value["error"]["code"], "invalidRequest");
}

#[tokio::test]
async fn automation_http_cannot_supply_model_completion_or_wake_decisions() {
    let state = test_state(false).await;
    let store = test_agent_execution_store().await;
    state
        .app
        .install_session_automation(store.automation_store(), Arc::new(TestAutomationSkills))
        .unwrap();
    for action in [
        AutomationAction::Goal {
            action: GoalAction::Complete {
                goal_id: "g1".into(),
                objective_revision: 1,
                evidence: "forged".into(),
            },
        },
        AutomationAction::Loop {
            action: LoopAction::Decide {
                loop_id: "l1".into(),
                config_revision: 1,
                decision: LoopWakeDecision::Continue {
                    delay_minutes: 1,
                    reason: "forged".into(),
                },
            },
        },
    ] {
        let response = execute_automation_handler(
            State(state.clone()),
            axum::extract::Extension(test_auth()),
            Json(AutomationRequest {
                session_id: "mock-session".into(),
                action,
            }),
        )
        .await
        .unwrap_err()
        .into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "invalidRequest");
    }
    let snapshot = session_automation_handler(
        State(state.clone()),
        axum::extract::Extension(test_auth()),
        Json(talos_types::SessionAutomationReadRequest {
            session_id: "mock-session".into(),
        }),
    )
    .await
    .unwrap();
    assert!(expect_success(snapshot.0).goal.is_none());
    let missing = session_automation_handler(
        State(state),
        axum::extract::Extension(test_auth()),
        Json(talos_types::SessionAutomationReadRequest {
            session_id: "not-owned".into(),
        }),
    )
    .await
    .unwrap_err()
    .into_response();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}
