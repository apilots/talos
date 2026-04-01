use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::process::Command;
use tokio::time::timeout;

use crate::runtimes::{resolve_base_dir, resolve_path};
use crate::ToolContext;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 2000;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

fn default_limit() -> usize {
    DEFAULT_LIMIT
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFilesRequest {
    pub pattern: String,
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrepFilesResponse {
    pub content: String,
    pub matches_found: usize,
}

pub async fn run(
    request: &GrepFilesRequest,
    context: &ToolContext,
) -> TalosResult<GrepFilesResponse> {
    let pattern = request.pattern.trim();
    if pattern.is_empty() {
        return Err(TalosError::ToolError(
            "pattern must not be empty".to_string(),
        ));
    }
    if request.limit == 0 {
        return Err(TalosError::ToolError(
            "limit must be greater than zero".to_string(),
        ));
    }

    let base_dir = resolve_base_dir(context)?;
    let search_path = request
        .path
        .as_deref()
        .map(|path| resolve_path(&base_dir, path))
        .unwrap_or_else(|| base_dir.clone());

    verify_path_exists(&search_path).await?;

    let include = request
        .include
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let limit = request.limit.min(MAX_LIMIT);
    let search_results = run_rg_search(pattern, include, &search_path, limit, &base_dir).await?;

    if search_results.is_empty() {
        Ok(GrepFilesResponse {
            content: "No matches found.".to_string(),
            matches_found: 0,
        })
    } else {
        Ok(GrepFilesResponse {
            matches_found: search_results.len(),
            content: search_results.join("\n"),
        })
    }
}

async fn verify_path_exists(path: &Path) -> TalosResult<()> {
    tokio::fs::metadata(path).await.map_err(|err| {
        TalosError::ToolError(format!("unable to access `{}`: {}", path.display(), err))
    })?;
    Ok(())
}

async fn run_rg_search(
    pattern: &str,
    include: Option<&str>,
    search_path: &Path,
    limit: usize,
    cwd: &Path,
) -> TalosResult<Vec<String>> {
    let mut command = Command::new("rg");
    command
        .current_dir(cwd)
        .arg("--files-with-matches")
        .arg("--sortr=modified")
        .arg("--regexp")
        .arg(pattern)
        .arg("--no-messages");

    if let Some(glob) = include {
        command.arg("--glob").arg(glob);
    }

    command.arg("--").arg(search_path);

    let output = timeout(COMMAND_TIMEOUT, command.output())
        .await
        .map_err(|_| TalosError::ToolError("rg timed out after 30 seconds".to_string()))?
        .map_err(|err| {
            TalosError::ToolError(format!(
                "failed to launch rg: {}. Ensure ripgrep is installed and on PATH.",
                err
            ))
        })?;

    match output.status.code() {
        Some(0) => Ok(parse_results(&output.stdout, limit)),
        Some(1) => Ok(Vec::new()),
        _ => Err(TalosError::ToolError(format!(
            "rg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))),
    }
}

fn parse_results(stdout: &[u8], limit: usize) -> Vec<String> {
    let mut results = Vec::new();
    for line in stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Ok(text) = std::str::from_utf8(line) {
            if text.is_empty() {
                continue;
            }
            results.push(text.to_string());
            if results.len() == limit {
                break;
            }
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn rg_available() -> bool {
        std::process::Command::new("rg")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    #[tokio::test]
    async fn test_grep_runtime_returns_matches() {
        if !rg_available() {
            return;
        }

        let dir = tempdir().expect("temp dir should create");
        tokio::fs::write(dir.path().join("a.txt"), "alpha beta")
            .await
            .expect("file should write");
        tokio::fs::write(dir.path().join("b.txt"), "gamma")
            .await
            .expect("file should write");

        let mut config = talos_config::AgentKernelConfig::default();
        config.memory.workspace_dir = dir.path().to_string_lossy().to_string();
        let context = crate::ToolContext::new_with_kernel_config(
            "session",
            "user",
            Some(std::sync::Arc::new(config)),
        );

        let response = run(
            &GrepFilesRequest {
                pattern: "alpha".to_string(),
                include: None,
                path: None,
                limit: 10,
            },
            &context,
        )
        .await
        .expect("search should succeed");

        assert_eq!(response.matches_found, 1);
        assert!(response.content.contains("a.txt"));
    }
}
