use super::*;
use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::runtimes::task_state_types::StructuredQuestionStatus;
use uuid::Uuid;

#[tokio::test]
async fn ask_user_question_records_pending_question() {
    let tool = AskUserQuestionTool;
    let session_id = format!("ask-user-{}", Uuid::new_v4());

    let payload = tool
        .execute_payload(
            serde_json::json!({
                "header": "Execution Mode",
                "question": "How should the agent proceed?",
                "options": [
                    { "id": "plan", "label": "Plan", "description": "Think first" },
                    { "id": "direct", "label": "Direct", "description": "Act now" }
                ]
            }),
            ToolContext::new(&session_id, "user-1"),
        )
        .await
        .expect("ask_user_question should succeed");

    let ToolOutputPayload::Function {
        structured_content, ..
    } = payload
    else {
        panic!("expected function payload");
    };
    assert_eq!(
        structured_content
            .as_ref()
            .and_then(|value| value.get("awaiting_user_response"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let record = TaskStateRuntime::global()
        .latest_question(&session_id)
        .expect("question lookup should succeed")
        .expect("question should be stored");
    assert_eq!(record.status, StructuredQuestionStatus::Pending);
    assert_eq!(record.request.header, "Execution Mode");
}
