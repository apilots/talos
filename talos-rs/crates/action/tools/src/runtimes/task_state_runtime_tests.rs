use super::*;
use uuid::Uuid;

fn todo_item(id: &str, content: &str, status: TodoStatus) -> TodoItem {
    TodoItem {
        id: id.to_string(),
        content: content.to_string(),
        status,
    }
}

#[test]
fn replace_todos_updates_runtime_and_shared_state() {
    let runtime = TaskStateRuntime::global();
    let session_id = format!("session-{}", Uuid::new_v4());

    let snapshot = runtime
        .replace_todos(
            &session_id,
            vec![
                todo_item("todo-1", "Inspect runtime", TodoStatus::InProgress),
                todo_item("todo-2", "Write tests", TodoStatus::Pending),
            ],
        )
        .expect("replace should succeed");

    assert_eq!(snapshot.items.len(), 2);
    let shared = get_session_tool_state(&session_id).tasks.list();
    assert_eq!(shared.len(), 2);
    assert!(shared[0].content.contains("in_progress"));
}

#[test]
fn enter_and_exit_plan_mode_updates_shared_permission_state() {
    let runtime = TaskStateRuntime::global();
    let session_id = format!("session-{}", Uuid::new_v4());

    assert_eq!(runtime.enter_plan_mode(&session_id), PermissionMode::Plan);
    assert_eq!(
        runtime.current_permission_mode(&session_id),
        PermissionMode::Plan
    );
    assert_eq!(runtime.exit_plan_mode(&session_id), PermissionMode::Default);
}
