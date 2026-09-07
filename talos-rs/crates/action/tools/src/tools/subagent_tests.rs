use std::sync::{Arc, Mutex};

use serde_json::json;
use talos_types::{
    AgentTreeSnapshot, AgentTurnRef, FollowUpSubagentRequest, InterruptSubagentRequest,
    ListSubagentsRequest, SendMessageResult, SendSubagentMessageRequest, SpawnSubagentRequest,
    SubagentOutcome, SubagentRef, SubagentSnapshot, SubagentSpawnResult, WaitSubagentRequest,
};

use super::*;
use crate::{AgentControl, AgentControlFuture};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Spawn(SpawnSubagentRequest),
    SendMessage(SendSubagentMessageRequest),
    Wait(WaitSubagentRequest),
    Interrupt(InterruptSubagentRequest),
    FollowUp(FollowUpSubagentRequest),
    List(ListSubagentsRequest),
}

#[derive(Default)]
struct RecordingControl {
    calls: Mutex<Vec<Call>>,
    send_error: Mutex<Option<TalosError>>,
}

impl AgentControl for RecordingControl {
    fn spawn(&self, request: SpawnSubagentRequest) -> AgentControlFuture<'_, SubagentSpawnResult> {
        let result = SubagentSpawnResult {
            subagent: SubagentRef {
                path: "/root/research".parse().unwrap(),
                session_id: "child-1".to_string(),
                parent: request.caller.turn_ref(),
            },
            agent_id: "default".to_string(),
            turn: AgentTurnRef {
                session_id: "child-1".to_string(),
                turn_id: "child-turn".to_string(),
            },
        };
        self.calls.lock().unwrap().push(Call::Spawn(request));
        Box::pin(async move { Ok(result) })
    }

    fn wait(&self, request: WaitSubagentRequest) -> AgentControlFuture<'_, SubagentSnapshot> {
        self.calls.lock().unwrap().push(Call::Wait(request));
        Box::pin(async move {
            Ok(SubagentSnapshot {
                subagent: child_ref(),
                agent_id: "reviewer".to_string(),
                latest_update: None,
                outcome: SubagentOutcome::Running {
                    turn_id: "child-turn".to_string(),
                },
            })
        })
    }

    fn send_message(
        &self,
        request: SendSubagentMessageRequest,
    ) -> AgentControlFuture<'_, SendMessageResult> {
        let error = self.send_error.lock().unwrap().take();
        let result = SendMessageResult {
            subagent: child_ref(),
            agent_id: "default".to_string(),
            turn: AgentTurnRef {
                session_id: "child-1".to_string(),
                turn_id: "child-turn".to_string(),
            },
            accepted: true,
        };
        self.calls.lock().unwrap().push(Call::SendMessage(request));
        Box::pin(async move {
            if let Some(error) = error {
                Err(error)
            } else {
                Ok(result)
            }
        })
    }

    fn interrupt(
        &self,
        request: InterruptSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        let subagent = child_ref();
        self.calls.lock().unwrap().push(Call::Interrupt(request));
        Box::pin(async move {
            Ok(SubagentSnapshot {
                subagent,
                agent_id: "reviewer".to_string(),
                latest_update: None,
                outcome: SubagentOutcome::Cancelling {
                    turn_id: "child-turn".to_string(),
                },
            })
        })
    }

    fn follow_up(
        &self,
        request: FollowUpSubagentRequest,
    ) -> AgentControlFuture<'_, SubagentSnapshot> {
        let subagent = child_ref();
        self.calls.lock().unwrap().push(Call::FollowUp(request));
        Box::pin(async move {
            Ok(SubagentSnapshot {
                subagent,
                agent_id: "reviewer".to_string(),
                latest_update: None,
                outcome: SubagentOutcome::Completed {
                    turn_id: "follow-up-turn".to_string(),
                    output: "done".to_string(),
                    truncated: false,
                },
            })
        })
    }

    fn list(&self, request: ListSubagentsRequest) -> AgentControlFuture<'_, AgentTreeSnapshot> {
        self.calls.lock().unwrap().push(Call::List(request));
        Box::pin(async {
            Ok(AgentTreeSnapshot {
                children: Vec::new(),
            })
        })
    }
}

fn child_ref() -> SubagentRef {
    SubagentRef {
        path: "/root/research".parse().unwrap(),
        session_id: "child-1".to_string(),
        parent: AgentTurnRef {
            session_id: "parent-1".to_string(),
            turn_id: "parent-turn".to_string(),
        },
    }
}

async fn execute(
    name: &str,
    input: Value,
    control: Arc<RecordingControl>,
) -> TalosResult<ToolOutputPayload> {
    let tool = subagent_tools()
        .into_iter()
        .find(|tool| tool.name() == name)
        .unwrap();
    tool.execute_payload(
        input,
        ToolContext::new("parent-1", "user-1")
            .with_turn_id("parent-turn")
            .with_agent_id("default")
            .with_agent_control(control),
    )
    .await
}

#[tokio::test]
async fn all_tools_delegate_with_trusted_parent_identity() {
    let control = Arc::new(RecordingControl::default());
    let outputs = [
        execute(
            "spawn_agent",
            json!({ "taskName": "research", "prompt": "Review" }),
            control.clone(),
        )
        .await
        .unwrap(),
        execute(
            "send_message",
            json!({
                "target": { "type": "directChild", "name": "research" },
                "message": "Focus on tests"
            }),
            control.clone(),
        )
        .await
        .unwrap(),
        execute(
            "wait_agent",
            json!({
                "target": { "type": "directChild", "name": "research" },
                "timeoutMs": 10
            }),
            control.clone(),
        )
        .await
        .unwrap(),
        execute(
            "interrupt_agent",
            json!({ "target": { "type": "directChild", "name": "research" } }),
            control.clone(),
        )
        .await
        .unwrap(),
        execute(
            "followup_task",
            json!({
                "target": { "type": "directChild", "name": "research" },
                "prompt": "Review tests"
            }),
            control.clone(),
        )
        .await
        .unwrap(),
        execute("list_agents", json!({}), control.clone())
            .await
            .unwrap(),
    ];

    let results = outputs
        .iter()
        .map(|output| {
            serde_json::from_str::<CollaborationToolResult>(output.content())
                .expect("decode collaboration result")
        })
        .collect::<Vec<_>>();
    assert!(matches!(results[0], CollaborationToolResult::SpawnAgent(_)));
    assert!(matches!(
        results[1],
        CollaborationToolResult::SendMessage(_)
    ));
    assert!(matches!(results[2], CollaborationToolResult::WaitAgent(_)));
    assert!(matches!(
        results[3],
        CollaborationToolResult::InterruptAgent(_)
    ));
    assert!(matches!(
        results[4],
        CollaborationToolResult::FollowupTask(_)
    ));
    assert!(matches!(results[5], CollaborationToolResult::ListAgents(_)));

    let calls = control.calls.lock().unwrap();
    assert!(matches!(
        &calls[0],
        Call::Spawn(request)
            if request.caller.session_id == "parent-1"
                && request.caller.turn_id == "parent-turn"
                && request.caller.agent_id == "default"
    ));
    assert!(matches!(
        &calls[1],
        Call::SendMessage(request)
            if request.caller.session_id == "parent-1"
                && request.target.as_str() == "research"
                && request.message == "Focus on tests"
    ));
    assert!(matches!(
        &calls[2],
        Call::Wait(request)
            if request.caller.session_id == "parent-1"
                && request.target.as_str() == "research"
                && request.timeout_ms == 10
    ));
    assert!(matches!(
        &calls[3],
        Call::Interrupt(request) if request.caller.session_id == "parent-1"
    ));
    assert!(matches!(
        &calls[4],
        Call::FollowUp(request) if request.caller.session_id == "parent-1"
    ));
    assert!(matches!(
        &calls[5],
        Call::List(request) if request.caller.session_id == "parent-1"
    ));
}

#[tokio::test]
async fn turn_bound_tools_require_a_trusted_turn() {
    let control = Arc::new(RecordingControl::default());
    let tool = subagent_tools()
        .into_iter()
        .find(|tool| tool.name() == "spawn_agent")
        .unwrap();

    let error = tool
        .execute_payload(
            json!({ "taskName": "research", "prompt": "Review" }),
            ToolContext::new("parent-1", "user-1")
                .with_agent_id("default")
                .with_agent_control(control.clone()),
        )
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ToolExecutionFailed));
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn turn_bound_tools_require_a_trusted_agent() {
    let control = Arc::new(RecordingControl::default());
    let tool = subagent_tools()
        .into_iter()
        .find(|tool| tool.name() == "spawn_agent")
        .unwrap();

    let error = tool
        .execute_payload(
            json!({ "taskName": "research", "prompt": "Review" }),
            ToolContext::new("parent-1", "user-1")
                .with_turn_id("parent-turn")
                .with_agent_control(control.clone()),
        )
        .await
        .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ToolExecutionFailed));
    assert!(control.calls.lock().unwrap().is_empty());
}

#[test]
fn spawn_agent_schema_only_exposes_task_name_and_prompt() {
    let schema = subagent_tools()
        .into_iter()
        .find(|tool| tool.name() == "spawn_agent")
        .unwrap()
        .parameters_schema()
        .unwrap();
    let properties = schema["properties"].as_object().unwrap();

    assert_eq!(
        properties.keys().cloned().collect::<Vec<_>>(),
        ["prompt".to_string(), "taskName".to_string()]
    );
    assert_eq!(schema["required"], json!(["prompt", "taskName"]));
    assert_eq!(schema["additionalProperties"], false);
}

#[test]
fn send_message_schema_is_strongly_typed_and_bounded() {
    let schema = subagent_tools()
        .into_iter()
        .find(|tool| tool.name() == "send_message")
        .unwrap()
        .parameters_schema()
        .unwrap();

    assert_eq!(schema["required"], json!(["message", "target"]));
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["message"]["maxLength"], 32768);
    assert_eq!(
        schema["properties"]["target"]["$ref"],
        "#/definitions/AgentTarget"
    );
}

#[tokio::test]
async fn spawn_agent_rejects_model_selected_agent_id() {
    let control = Arc::new(RecordingControl::default());

    let error = execute(
        "spawn_agent",
        json!({
            "taskName": "research",
            "agentId": "reviewer",
            "prompt": "Review"
        }),
        control.clone(),
    )
    .await
    .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ToolExecutionFailed));
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_message_rejects_unknown_fields() {
    let control = Arc::new(RecordingControl::default());

    let error = execute(
        "send_message",
        json!({
            "target": { "type": "directChild", "name": "research" },
            "message": "Review",
            "agentId": "reviewer"
        }),
        control.clone(),
    )
    .await
    .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ToolExecutionFailed));
    assert!(control.calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_message_propagates_the_control_error() {
    let control = Arc::new(RecordingControl::default());
    *control.send_error.lock().unwrap() = Some(TalosError::new(
        talos_error::ErrorCode::Conflict,
        "message input closed".to_string(),
    ));

    let error = execute(
        "send_message",
        json!({
            "target": { "type": "directChild", "name": "research" },
            "message": "Review"
        }),
        control,
    )
    .await
    .unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::Conflict));
    assert_eq!(error.diagnostic_message(), "message input closed");
}

#[test]
fn collaboration_payload_rejects_the_complete_oversized_result() {
    let result = CollaborationToolResult::WaitAgent(SubagentSnapshot {
        subagent: child_ref(),
        agent_id: "reviewer".to_string(),
        latest_update: None,
        outcome: SubagentOutcome::Completed {
            turn_id: "child-turn".to_string(),
            output: "x".repeat(MAX_COLLABORATION_RESULT_BYTES),
            truncated: false,
        },
    });

    let error = collaboration_payload(result).unwrap_err();

    assert!(error.is_code(talos_error::ErrorCode::ToolExecutionFailed));
}
