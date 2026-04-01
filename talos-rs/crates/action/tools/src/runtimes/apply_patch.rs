use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use talos_tools_sandbox::{
    ApprovableTool, SandboxAwareTool, ToolApprovalRequirement, ToolOrchestrator,
    ToolOrchestratorConfig, ToolSandboxMode,
};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::runtimes::resolve_base_dir;
use crate::ToolContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchRequest {
    pub input: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyPatchResponse {
    pub output: String,
}

struct ApplyPatchPlanner;

impl ApprovableTool<ApplyPatchRequest> for ApplyPatchPlanner {
    fn approval_requirement(&self, _request: &ApplyPatchRequest) -> ToolApprovalRequirement {
        ToolApprovalRequirement::Skip
    }
}

impl SandboxAwareTool<ApplyPatchRequest> for ApplyPatchPlanner {
    fn sandbox_mode(
        &self,
        _request: &ApplyPatchRequest,
        _config: &ToolOrchestratorConfig,
    ) -> ToolSandboxMode {
        ToolSandboxMode::WorkspaceWrite
    }
}

pub async fn run(
    request: &ApplyPatchRequest,
    context: &ToolContext,
) -> TalosResult<ApplyPatchResponse> {
    if request.input.trim().is_empty() {
        return Err(TalosError::ToolError(
            "input must contain a patch body".to_string(),
        ));
    }

    if !context.pre_approved() {
        let orchestrator = ToolOrchestrator::new(ToolOrchestratorConfig::default());
        let plan = orchestrator
            .plan(&ApplyPatchPlanner, request)
            .map_err(|err| TalosError::ToolError(err.to_string()))?;
        plan.ensure_approved()
            .map_err(|err| TalosError::ToolError(err.to_string()))?;
    }

    let base_dir = resolve_base_dir(context)?;
    let mut command = Command::new("apply_patch");
    command
        .current_dir(base_dir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| TalosError::ToolError(format!("failed to launch apply_patch: {}", err)))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(request.input.as_bytes())
            .await
            .map_err(|err| {
                TalosError::ToolError(format!("failed to write patch to stdin: {}", err))
            })?;
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|err| TalosError::ToolError(format!("failed to execute apply_patch: {}", err)))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(ApplyPatchResponse {
            output: if stdout.is_empty() {
                "Patch applied successfully.".to_string()
            } else {
                stdout
            },
        })
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let message = if !stderr.is_empty() { stderr } else { stdout };
        Err(TalosError::ToolError(if message.is_empty() {
            "apply_patch failed".to_string()
        } else {
            format!("apply_patch failed: {}", message)
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn apply_patch_available() -> bool {
        std::process::Command::new("apply_patch")
            .arg("--help")
            .output()
            .map(|output| output.status.success() || !output.stderr.is_empty())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn test_apply_patch_runtime_updates_file() {
        if !apply_patch_available() {
            return;
        }

        let dir = tempdir().expect("temp dir should create");
        let file_path = dir.path().join("demo.txt");
        tokio::fs::write(&file_path, "before\n")
            .await
            .expect("seed file should write");

        let mut config = talos_config::AgentKernelConfig::default();
        config.memory.workspace_dir = dir.path().to_string_lossy().to_string();
        let context =
            crate::ToolContext::new_with_kernel_config("session", "user", Some(Arc::new(config)));

        let patch = r#"*** Begin Patch
*** Update File: demo.txt
@@
-before
+after
*** End Patch"#;

        let response = run(
            &ApplyPatchRequest {
                input: patch.to_string(),
            },
            &context,
        )
        .await
        .expect("apply_patch should succeed");

        let content = tokio::fs::read_to_string(&file_path)
            .await
            .expect("updated file should read");
        assert!(response.output.contains("demo.txt"));
        assert!(content.contains("after"));
    }
}
