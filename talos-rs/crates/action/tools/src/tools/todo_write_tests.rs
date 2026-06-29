use super::*;
use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::state::get_session_tool_state;
use uuid::Uuid;

#[tokio::test]
async fn todo_write_updates_session_todo_state() {
    let tool = TodoWriteTool;
    let session_id = format!("todo-write-{}", Uuid::new_v4());
    let payload = tool
        .execute_payload(
            serde_json::json!({
                "todos": [
                    { "id": "t1", "content": "Inspect tool state", "status": "in_progress" },
                    { "id": "t2", "content": "Add tests", "status": "pending" }
                ]
            }),
            ToolContext::new(&session_id, "user-1"),
        )
        .await
        .expect("todo_write should succeed");

    let ToolOutputPayload::Function {
        structured_content, ..
    } = payload
    else {
        panic!("expected function payload");
    };
    assert_eq!(
        structured_content
            .as_ref()
            .and_then(|value| value.get("items"))
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    let snapshot = TaskStateRuntime::global()
        .list_todos(&session_id)
        .expect("todo snapshot should exist");
    assert_eq!(snapshot.items.len(), 2);
    assert_eq!(snapshot.items[0].id, "t1");

    let compatibility = get_session_tool_state(&session_id).tasks.list();
    assert_eq!(compatibility.len(), 2);
    assert!(compatibility[0].content.contains("Inspect tool state"));
}
