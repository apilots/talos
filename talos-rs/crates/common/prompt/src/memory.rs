use crate::{bootstrap, PromptBuilder};

pub(super) fn policy_payload() -> String {
    [
        "User Workspace Memory is scoped to the current agent and user and is shared across their Sessions.",
        "Use UserWorkspaceMemoryUpdate for explicit remember, correct, or forget requests and for durable preferences, decisions, constraints, commitments, or lessons.",
        "Do not record logs, transient status, recoverable repository facts, speculation, secrets, or duplicates.",
        "Update entries by stable key and remove stale entries instead of accumulating history.",
    ]
    .join("\n\n")
}

pub(super) fn prompt_payload(builder: &PromptBuilder<'_>) -> String {
    let Some(context) = builder.user_workspace_memory.as_ref() else {
        return "file_state: missing\nversion_token: none".to_string();
    };
    let content = bootstrap::truncate_chars(&context.content, talos_types::MEMORY_FILE_MAX_CHARS);
    let version = context.version_token.as_deref().unwrap_or("none");
    if content.trim().is_empty() {
        format!(
            "file_state: {}\nversion_token: {version}",
            context.file_state.as_str()
        )
    } else {
        format!(
            "file_state: {}\nversion_token: {version}\n\n{}",
            context.file_state.as_str(),
            content.trim()
        )
    }
}
