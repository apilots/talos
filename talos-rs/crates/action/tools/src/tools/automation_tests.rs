use std::sync::{Arc, Mutex};

use serde_json::json;
use talos_types::{
    AutomationControlResult, AutomationEffect, GoalAction, LoopSchedule, LoopStatus,
    SessionAutomationSnapshot, SessionLoop,
};

use super::*;
use crate::{AutomationControl, AutomationControlFuture, ToolRegistry};

struct RecordingControl {
    requests: Mutex<Vec<AutomationModelRequest>>,
    result: AutomationControlResult,
}

impl AutomationControl for RecordingControl {
    fn execute(&self, request: AutomationModelRequest) -> AutomationControlFuture<'_> {
        self.requests.lock().unwrap().push(request);
        Box::pin(async { Ok(self.result.clone()) })
    }
}

fn control(effect: AutomationEffect) -> Arc<RecordingControl> {
    Arc::new(RecordingControl {
        requests: Mutex::new(Vec::new()),
        result: AutomationControlResult {
            created_object: None,
            effect,
            bound_origin: None,
            snapshot: SessionAutomationSnapshot {
                session_id: "session".into(),
                goal: None,
                loops: Vec::new(),
            },
        },
    })
}

fn context(control: Arc<dyn AutomationControl>) -> ToolContext {
    runtime_context().with_automation_control(control)
}

fn runtime_context() -> ToolContext {
    let mut context = ToolContext::new("session", "workspace-user")
        .with_turn_id("turn")
        .with_agent_id("agent");
    context.bind_current_call_id("runtime-call");
    context
}

#[tokio::test]
async fn forwards_only_runtime_caller_and_preserves_pending_settlement() {
    let control = control(AutomationEffect::PendingSettlement);
    let action = AutomationAction::Goal {
        action: GoalAction::Complete {
            goal_id: "goal".into(),
            objective_revision: 3,
            evidence: "Tests passed".into(),
        },
    };
    let payload = AutomationTool
        .execute_payload(
            serde_json::to_value(&action).unwrap(),
            context(control.clone()),
        )
        .await
        .unwrap();
    assert_eq!(
        *control.requests.lock().unwrap(),
        vec![AutomationModelRequest {
            caller: AgentCaller {
                session_id: "session".into(),
                turn_id: "turn".into(),
                agent_id: "agent".into(),
            },
            call_id: "runtime-call".into(),
            action,
        }]
    );
    assert_eq!(
        payload.structured_content(),
        Some(&serde_json::to_value(&control.result).unwrap())
    );
}

#[tokio::test]
async fn invocation_port_overrides_registry_default() {
    let registry = ToolRegistry::new();
    let default = control(AutomationEffect::Applied);
    let invocation = control(AutomationEffect::PendingSettlement);
    registry.set_automation_control(default.clone());
    let mut context = registry
        .build_context("session", "user", None)
        .with_turn_id("turn")
        .with_agent_id("agent")
        .with_automation_control(invocation.clone());
    context.bind_current_call_id("invocation-call");
    AutomationTool
        .execute_payload(
            json!({"target": "loop", "action": {"type": "list"}}),
            context,
        )
        .await
        .unwrap();
    assert!(default.requests.lock().unwrap().is_empty());
    assert_eq!(invocation.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn registry_dispatch_reaches_the_same_typed_control_port() {
    let registry = ToolRegistry::new();
    registry.register(Arc::new(AutomationTool));
    let control = control(AutomationEffect::Applied);
    registry.set_automation_control(control.clone());
    let invocation = registry
        .build_function_invocation(
            "call",
            "automation",
            json!({"target": "goal", "action": {"type": "read"}}),
        )
        .unwrap();
    let context = registry
        .build_context("session", "user", None)
        .with_turn_id("turn")
        .with_agent_id("agent");
    let crate::ToolDispatchOutcome::Executed { payload } = registry
        .execute_invocation_with_context(invocation, context)
        .await
        .unwrap()
    else {
        panic!("Automation must not add an independent approval step");
    };
    assert_eq!(payload.structured_content().unwrap()["effect"], "applied");
    assert_eq!(control.requests.lock().unwrap().len(), 1);
    assert_eq!(control.requests.lock().unwrap()[0].call_id, "call");
}

#[tokio::test]
async fn schema_feedback_allows_correcting_real_automation_arguments_through_registry() {
    let loop_create = json!({
        "target": "loop",
        "action": {
            "type": "create", "prompt": "Check CI", "schedule": {"type": "modelDecided"}
        }
    });
    let mut missing_operation = loop_create.clone();
    missing_operation["action"]
        .as_object_mut()
        .unwrap()
        .remove("type")
        .unwrap();
    let mut invalid_schedule = loop_create.clone();
    invalid_schedule["action"]["schedule"]["type"] = json!("invalidSchedule");
    let goal_read = json!({"target": "goal", "action": {"type": "read"}});
    let mut extra_read = goal_read.clone();
    extra_read["action"]["read"] = json!({});

    for (invalid, corrected, field) in [
        (missing_operation, loop_create.clone(), "/action/type"),
        (invalid_schedule, loop_create, "/action/schedule/type"),
        (extra_read, goal_read, "/action/read"),
    ] {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(AutomationTool));
        let control = control(AutomationEffect::Applied);
        let invocation = registry
            .build_function_invocation("invalid-input", "automation", invalid)
            .unwrap();
        let failure = registry
            .execute_invocation_with_context(invocation, context(control.clone()))
            .await
            .unwrap_err();
        assert!(failure.error().is_code(ErrorCode::InvalidRequest));
        assert!(std::error::Error::source(failure.error())
            .unwrap()
            .is::<serde_json::Error>());
        let message = failure.error().to_api_error().message;
        eprintln!("schema diagnostic ({field}): {message}");
        assert!(message.contains(field), "{message}");
        assert!(!message.contains("invalidSchedule"), "{message}");
        let crate::ToolDispatchOutcome::Executed { payload } = failure.into_outcome() else {
            panic!("Invalid arguments must use the existing tool-result projection");
        };
        assert_eq!(
            payload.structured_content().unwrap(),
            &json!({
                "error": message,
                "error_kind": "validation_failure",
                "gate_source": "tool_semantic"
            })
        );
        assert!(control.requests.lock().unwrap().is_empty());

        let invocation = registry
            .build_function_invocation("corrected-input", "automation", corrected.clone())
            .unwrap();
        registry
            .execute_invocation_with_context(invocation, context(control.clone()))
            .await
            .unwrap();
        let requests = control.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            serde_json::to_value(&requests[0].action).unwrap(),
            corrected
        );
    }
}

#[tokio::test]
async fn distinct_runtime_calls_in_one_turn_keep_distinct_create_identities() {
    let control = control(AutomationEffect::Applied);
    for call_id in ["first-call", "second-call"] {
        let mut context = context(control.clone());
        context.bind_current_call_id(call_id);
        AutomationTool
            .execute_payload(
                json!({
                    "target": "loop", "action": {
                        "type": "create", "prompt": "Check CI", "schedule": {"type": "modelDecided"}
                    }
                }),
                context,
            )
            .await
            .unwrap();
    }
    let requests = control.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].caller, requests[1].caller);
    assert_eq!(
        requests
            .iter()
            .map(|request| request.call_id.as_str())
            .collect::<Vec<_>>(),
        ["first-call", "second-call"]
    );
}

#[test]
fn large_model_view_is_explicitly_shortened_without_losing_effect() {
    let mut result = control(AutomationEffect::PendingSettlement).result.clone();
    let origin = talos_types::AutomationTurnOrigin::Loop {
        loop_id: "bound-loop-with-unique-id".repeat(4),
        occurrence_id: "bound-occurrence-with-unique-id".repeat(4),
        config_revision: 42,
    };
    result.bound_origin = Some(origin.clone());
    let created = talos_types::AutomationObjectRef::Loop {
        loop_id: "owner-issued-created-loop".into(),
    };
    result.created_object = Some(created.clone());
    result.snapshot.loops = (0..128)
        .map(|index| SessionLoop {
            id: format!("loop-{index}"),
            session_id: "session".into(),
            prompt: "Task ".repeat(1000),
            schedule: LoopSchedule::ModelDecided {},
            config_revision: 1,
            status: LoopStatus::Active,
            stop_reason: None,
            reason: None,
            next_due_at: None,
            latest_turn_id: None,
            created_at: 0,
            updated_at: 0,
        })
        .collect();
    let payload = model_result(result).unwrap();
    let value = payload.structured_content().unwrap();
    assert_eq!(value["effect"], "pendingSettlement");
    assert_eq!(value["boundOrigin"], serde_json::to_value(origin).unwrap());
    assert_eq!(
        value["createdObject"],
        serde_json::to_value(created).unwrap()
    );
    assert_eq!(value["truncated"], true);
    assert!(value["notice"].as_str().unwrap().contains("shortened"));
    assert!(value.to_string().len() <= MAX_RESULT_BYTES);
}
