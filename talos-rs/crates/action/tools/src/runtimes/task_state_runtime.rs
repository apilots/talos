use std::sync::{Arc, OnceLock, RwLock};

use crate::runtimes::task_state_types::{TodoItem, TodoListSnapshot, TodoStatus};
use crate::state::{get_session_tool_state, TaskStateEntry};
use dashmap::DashMap;
use talos_error::{ErrorCode, TalosError, TalosResult};

#[derive(Debug, Default)]
struct SessionTaskState {
    todos: RwLock<Vec<TodoItem>>,
}

#[derive(Debug, Default)]
pub struct TaskStateRuntime {
    sessions: DashMap<String, Arc<SessionTaskState>>,
}

impl TaskStateRuntime {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<TaskStateRuntime> = OnceLock::new();
        INSTANCE.get_or_init(Self::default)
    }

    pub fn replace_todos(
        &self,
        session_id: &str,
        items: Vec<TodoItem>,
    ) -> TalosResult<TodoListSnapshot> {
        validate_todo_items(&items)?;
        let session = self.session_state(session_id);
        {
            let mut guard = session
                .todos
                .write()
                .map_err(|_| TalosError::internal("failed to acquire todo state write lock"))?;
            *guard = items.clone();
        }

        let compatibility_items = items
            .iter()
            .map(render_task_state_entry)
            .collect::<Vec<TaskStateEntry>>();
        get_session_tool_state(session_id)
            .tasks
            .replace(compatibility_items);

        Ok(TodoListSnapshot { items })
    }

    pub fn list_todos(&self, session_id: &str) -> TalosResult<TodoListSnapshot> {
        let session = self.session_state(session_id);
        let items = session
            .todos
            .read()
            .map_err(|_| TalosError::internal("failed to acquire todo state read lock"))?
            .clone();
        Ok(TodoListSnapshot { items })
    }

    fn session_state(&self, session_id: &str) -> Arc<SessionTaskState> {
        self.sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(SessionTaskState::default()))
            .clone()
    }
}

fn render_task_state_entry(item: &TodoItem) -> TaskStateEntry {
    TaskStateEntry {
        content: format!("[{}] {}", todo_status_label(item.status), item.content),
    }
}

fn todo_status_label(status: TodoStatus) -> &'static str {
    match status {
        TodoStatus::Pending => "pending",
        TodoStatus::InProgress => "in_progress",
        TodoStatus::Completed => "completed",
    }
}

fn validate_todo_items(items: &[TodoItem]) -> TalosResult<()> {
    if items.is_empty() {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "todos must contain at least one item",
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        if item.id.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "todo id cannot be empty",
            ));
        }
        if item.content.trim().is_empty() {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "todo content cannot be empty",
            ));
        }
        if !seen.insert(item.id.clone()) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                format!("duplicate todo id `{}`", item.id),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "task_state_runtime_tests.rs"]
mod tests;
