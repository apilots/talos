use std::sync::{Arc, OnceLock, RwLock};

use dashmap::DashMap;
use talos_error::{TalosError, TalosResult};
use uuid::Uuid;

use crate::runtimes::task_state_types::{
    StructuredQuestionAnswer, StructuredQuestionOption, StructuredQuestionRecord,
    StructuredQuestionRequest, StructuredQuestionStatus, TodoItem, TodoListSnapshot, TodoStatus,
};
use crate::state::{get_session_tool_state, PermissionMode, TaskStateEntry};

#[derive(Debug, Default)]
struct SessionTaskState {
    todos: RwLock<Vec<TodoItem>>,
    latest_question: RwLock<Option<StructuredQuestionRecord>>,
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
            let mut guard = session.todos.write().map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "todo state lock poisoned".to_string(),
                )
            })?;
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
            .map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "todo state lock poisoned".to_string(),
                )
            })?
            .clone();
        Ok(TodoListSnapshot { items })
    }

    pub fn enter_plan_mode(&self, session_id: &str) -> PermissionMode {
        let state = get_session_tool_state(session_id);
        state.permissions.set_mode(PermissionMode::Plan);
        state.permissions.mode()
    }

    pub fn exit_plan_mode(&self, session_id: &str) -> PermissionMode {
        let state = get_session_tool_state(session_id);
        state.permissions.set_mode(PermissionMode::Default);
        state.permissions.mode()
    }

    pub fn current_permission_mode(&self, session_id: &str) -> PermissionMode {
        get_session_tool_state(session_id).permissions.mode()
    }

    pub fn ask_question(
        &self,
        session_id: &str,
        header: String,
        question: String,
        options: Vec<StructuredQuestionOption>,
        allows_free_form: bool,
    ) -> TalosResult<StructuredQuestionRecord> {
        validate_question(&header, &question, &options)?;
        let session = self.session_state(session_id);
        let record = StructuredQuestionRecord {
            request: StructuredQuestionRequest {
                question_id: Uuid::new_v4().to_string(),
                header,
                question,
                options,
                allows_free_form,
            },
            status: StructuredQuestionStatus::Pending,
            answer: None,
        };

        {
            let mut guard = session.latest_question.write().map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "question state lock poisoned".to_string(),
                )
            })?;
            *guard = Some(record.clone());
        }

        // Host responders are not connected here; AskUserQuestion currently records
        // the pending structured question in-process, and completion resumes only
        // after a caller invokes `submit_question_answer`.

        Ok(record)
    }

    pub fn latest_question(
        &self,
        session_id: &str,
    ) -> TalosResult<Option<StructuredQuestionRecord>> {
        let session = self.session_state(session_id);
        session
            .latest_question
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "question state lock poisoned".to_string(),
                )
            })
    }

    pub fn submit_question_answer(
        &self,
        session_id: &str,
        answer: StructuredQuestionAnswer,
    ) -> TalosResult<StructuredQuestionRecord> {
        let session = self.session_state(session_id);
        let mut guard = session.latest_question.write().map_err(|_| {
            TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                "question state lock poisoned".to_string(),
            )
        })?;
        let record = guard.as_mut().ok_or_else(|| {
            TalosError::public(
                talos_error::ErrorCode::ResourceNotFound,
                "pending question".to_string(),
            )
        })?;
        if record.request.question_id != answer.question_id {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!(
                    "question `{}` is not pending in session `{session_id}`",
                    answer.question_id
                ),
            ));
        }

        if let Some(selected_option_id) = answer.selected_option_id.as_deref() {
            let option_exists = record
                .request
                .options
                .iter()
                .any(|option| option.id == selected_option_id);
            if !option_exists {
                return Err(TalosError::public(
                    talos_error::ErrorCode::InvalidRequest,
                    format!(
                        "option `{selected_option_id}` does not exist for question `{}`",
                        answer.question_id
                    ),
                ));
            }
        }

        if answer.selected_option_id.is_none()
            && answer
                .free_form_text
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "answer must contain either a selected option or free-form text".to_string(),
            ));
        }

        record.status = StructuredQuestionStatus::Answered;
        record.answer = Some(answer);
        Ok(record.clone())
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
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "todos must contain at least one item".to_string(),
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for item in items {
        if item.id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "todo id cannot be empty".to_string(),
            ));
        }
        if item.content.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "todo content cannot be empty".to_string(),
            ));
        }
        if !seen.insert(item.id.clone()) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("duplicate todo id `{}`", item.id),
            ));
        }
    }
    Ok(())
}

fn validate_question(
    header: &str,
    question: &str,
    options: &[StructuredQuestionOption],
) -> TalosResult<()> {
    if header.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "header cannot be empty".to_string(),
        ));
    }
    if question.trim().is_empty() {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "question cannot be empty".to_string(),
        ));
    }
    if !(2..=3).contains(&options.len()) {
        return Err(TalosError::public(
            talos_error::ErrorCode::InvalidRequest,
            "options must contain 2 or 3 entries".to_string(),
        ));
    }

    let mut seen = std::collections::BTreeSet::new();
    for option in options {
        if option.id.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "option id cannot be empty".to_string(),
            ));
        }
        if option.label.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "option label cannot be empty".to_string(),
            ));
        }
        if option.description.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                "option description cannot be empty".to_string(),
            ));
        }
        if !seen.insert(option.id.clone()) {
            return Err(TalosError::public(
                talos_error::ErrorCode::InvalidRequest,
                format!("duplicate option id `{}`", option.id),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "task_state_runtime_tests.rs"]
mod tests;
