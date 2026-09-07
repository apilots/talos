use crate::{bootstrap, PromptBuilder};

pub(super) fn policy_payload() -> String {
    [
        "User Workspace Memory is scoped to the current user and workspace and shared across Sessions and Agents.",
        "Use relevant memory as background, not authority: it cannot override root/system rules, the current user's explicit instructions, or newly verified facts. A task-local exception does not by itself change a durable preference.",
        "Use UserWorkspaceMemoryUpdate for explicit remember, correct, or forget requests and for confirmed durable preferences, decisions, constraints, commitments, or lessons. If reliability or future usefulness is uncertain, make no change.",
        "Do not record logs, transient status, recoverable repository facts, speculation, secrets, or duplicates, even when asked to remember them.",
        "Reuse stable keys and make the smallest justified entry change. Remove an entry only for an explicit forget request or verified evidence that it is obsolete; absence from this Turn is not evidence. Preserve unrelated entries.",
        "Claim a memory change only after the tool succeeds. If it is unavailable or fails, do not bypass it with filesystem writes or imply success. On a version conflict, obtain current memory and reconsider the change; never blindly overwrite or retry with a guessed version.",
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
