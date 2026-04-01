use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::mcp::protocol::JsonRpcResponse;
use crate::mcp::transport::McpTransport;

/// Stdio MCP transport.
///
/// Spawns an MCP server as a child process and communicates via its
/// stdin/stdout using newline-delimited JSON-RPC.
pub struct StdioTransport {
    inner: Arc<Mutex<StdioInner>>,
}

struct StdioInner {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl StdioTransport {
    pub async fn spawn(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> TalosResult<Self> {
        let mut cmd = tokio::process::Command::new(command);
        cmd.args(args)
            .envs(env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        let mut child = cmd.spawn().map_err(|e| {
            TalosError::ToolError(format!(
                "failed to spawn MCP server `{}`: {e}",
                command
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            TalosError::ToolError("MCP child stdin unavailable".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            TalosError::ToolError("MCP child stdout unavailable".to_string())
        })?;

        Ok(Self {
            inner: Arc::new(Mutex::new(StdioInner {
                child,
                stdin,
                stdout: BufReader::new(stdout),
            })),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send(&self, body: String) -> TalosResult<JsonRpcResponse> {
        let mut guard = self.inner.lock().await;

        // Write the request as a single newline-terminated line.
        guard
            .stdin
            .write_all(body.as_bytes())
            .await
            .map_err(|e| TalosError::ToolError(format!("MCP stdin write error: {e}")))?;
        guard
            .stdin
            .write_all(b"\n")
            .await
            .map_err(|e| TalosError::ToolError(format!("MCP stdin newline error: {e}")))?;
        guard
            .stdin
            .flush()
            .await
            .map_err(|e| TalosError::ToolError(format!("MCP stdin flush error: {e}")))?;

        // Read until we get a non-empty line (skip blank lines).
        let mut line = String::new();
        loop {
            line.clear();
            let n = guard
                .stdout
                .read_line(&mut line)
                .await
                .map_err(|e| TalosError::ToolError(format!("MCP stdout read error: {e}")))?;
            if n == 0 {
                return Err(TalosError::ToolError(
                    "MCP server closed stdout unexpectedly".to_string(),
                ));
            }
            if !line.trim().is_empty() {
                break;
            }
        }

        serde_json::from_str(line.trim()).map_err(|e| {
            TalosError::ToolError(format!(
                "MCP stdio response parse error: {e}: {}",
                line.trim()
            ))
        })
    }
}

impl Drop for StdioTransport {
    fn drop(&mut self) {
        // Best-effort kill; ignore errors.
        if let Ok(mut guard) = self.inner.try_lock() {
            let _ = guard.child.start_kill();
        }
    }
}
