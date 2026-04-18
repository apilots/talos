use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use talos_error::TalosResult;

use crate::runtimes::task_state_runtime::TaskStateRuntime;
use crate::runtimes::task_state_types::{TodoItem, TodoStatus};
use crate::spec::{ToolBehaviorFlags, ToolResultPolicy, ToolVisibility};
use crate::{decode_tool_input, Tool, ToolContext, ToolOutputPayload};

pub struct TodoWriteTool;

#[derive(Debug, Deserialize)]
struct TodoWriteRequest {
    todos: Vec<TodoWriteItem>,
}

#[derive(Debug, Deserialize)]
struct TodoWriteItem {
    id: String,
    content: String,
    status: TodoStatus,
}

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "TodoWrite"
    }

    fn internal_name(&self) -> &str {
        "todo_write"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["todo_write"]
    }

    fn description(&self) -> &str {
        "Update the todo list for the current session.\n\nUse this tool proactively for complex multi-step work to track progress, keep exactly one task in_progress, and update items immediately as work advances."
    }

    fn visibility(&self) -> ToolVisibility {
        ToolVisibility::DefaultVisible
    }

    fn behavior_flags(&self) -> ToolBehaviorFlags {
        ToolBehaviorFlags {
            read_only: false,
            destructive: false,
            concurrency_safe: false,
            requires_user_interaction: false,
        }
    }

    fn result_policy(&self) -> ToolResultPolicy {
        ToolResultPolicy {
            max_inline_bytes: 16 * 1024,
            persist_large_output: false,
            preview_lines: 50,
        }
    }

    fn parameters_schema(&self) -> Option<Value> {
        Some(serde_json::json!({
            "type": "object",
            "required": ["todos"],
            "properties": {
                "todos": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["id", "content", "status"],
                        "properties": {
                            "id": { "type": "string" },
                            "content": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            }
                        }
                    }
                }
            }
        }))
    }

    async fn execute_payload(
        &self,
        input: Value,
        context: ToolContext,
    ) -> TalosResult<ToolOutputPayload> {
        let request: TodoWriteRequest = decode_tool_input(input, self.name())?;
        let items = request
            .todos
            .into_iter()
            .map(|item| TodoItem {
                id: item.id,
                content: item.content,
                status: item.status,
            })
            .collect::<Vec<_>>();
        let snapshot = TaskStateRuntime::global().replace_todos(context.session_id(), items)?;
        ToolOutputPayload::success_json(&snapshot, "serialize todo_write response")
    }
}

#[cfg(test)]
mod tests {
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
}
