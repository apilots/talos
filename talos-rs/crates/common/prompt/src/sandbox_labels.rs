pub(super) fn sandbox_mode_label(mode: &talos_config::ToolSandboxModeConfig) -> &'static str {
    match mode {
        talos_config::ToolSandboxModeConfig::UseDefault => "use_default",
        talos_config::ToolSandboxModeConfig::ReadOnly => "read_only",
        talos_config::ToolSandboxModeConfig::WorkspaceWrite => "workspace_write",
        talos_config::ToolSandboxModeConfig::DangerFullAccess => "danger_full_access",
    }
}

pub(super) fn user_permission_mode_label(
    mode: &talos_config::ToolUserPermissionModeConfig,
) -> &'static str {
    match mode {
        talos_config::ToolUserPermissionModeConfig::AskForApproval => "ask_for_approval",
        talos_config::ToolUserPermissionModeConfig::ApproveForMe => "approve_for_me",
        talos_config::ToolUserPermissionModeConfig::FullAccess => "full_access",
    }
}
