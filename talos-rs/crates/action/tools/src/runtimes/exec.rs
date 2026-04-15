use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, Notify};

use crate::runtimes::{resolve_base_dir, resolve_path, truncate_prefix};
use crate::ToolContext;

const DEFAULT_EXEC_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_WRITE_STDIN_YIELD_TIME_MS: u64 = 250;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024;

fn default_exec_yield_time_ms() -> u64 {
    DEFAULT_EXEC_YIELD_TIME_MS
}

fn default_write_stdin_yield_time_ms() -> u64 {
    DEFAULT_WRITE_STDIN_YIELD_TIME_MS
}

fn default_login() -> bool {
    true
}

fn default_tty() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPermissions {
    #[default]
    UseDefault,
    RequireEscalated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecCommandRequest {
    pub cmd: String,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default = "default_login")]
    pub login: bool,
    #[serde(default = "default_tty")]
    pub tty: bool,
    #[serde(default = "default_exec_yield_time_ms")]
    pub yield_time_ms: u64,
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
    #[serde(default)]
    pub sandbox_permissions: SandboxPermissions,
    #[serde(default)]
    pub justification: Option<String>,
    #[serde(default)]
    pub prefix_rule: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteStdinRequest {
    pub session_id: i64,
    #[serde(default)]
    pub chars: String,
    #[serde(default = "default_write_stdin_yield_time_ms")]
    pub yield_time_ms: u64,
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecStatus {
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecResponse {
    pub session_id: Option<i64>,
    pub status: ExecStatus,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
}

struct ExecSession {
    session_id: i64,
    owner_session_id: String,
    max_output_bytes: usize,
    stdin: Mutex<Option<ChildStdin>>,
    state: Mutex<ExecSessionState>,
    notify: Notify,
}

#[derive(Default)]
struct ExecSessionState {
    output: String,
    delivered_bytes: usize,
    exit_code: Option<i32>,
}

struct ExecSessionManager {
    next_id: AtomicI64,
    sessions: DashMap<i64, Arc<ExecSession>>,
}

impl ExecSessionManager {
    fn global() -> &'static ExecSessionManager {
        static INSTANCE: OnceLock<ExecSessionManager> = OnceLock::new();
        INSTANCE.get_or_init(|| ExecSessionManager {
            next_id: AtomicI64::new(1),
            sessions: DashMap::new(),
        })
    }

    async fn open_session(
        &self,
        request: &ExecCommandRequest,
        context: &ToolContext,
    ) -> TalosResult<ExecResponse> {
        if request.cmd.trim().is_empty() {
            return Err(TalosError::ToolError("cmd cannot be empty".to_string()));
        }

        let base_dir = resolve_base_dir(context)?;
        let workdir = request
            .workdir
            .as_deref()
            .map(|path| resolve_path(&base_dir, path))
            .unwrap_or(base_dir);

        let mut command = build_command(request, &workdir)?;
        let mut child = command
            .spawn()
            .map_err(|err| TalosError::ToolError(format!("failed to execute command: {}", err)))?;

        let session_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        let session = Arc::new(ExecSession {
            session_id,
            owner_session_id: context.session_id().to_string(),
            max_output_bytes: context
                .kernel_config()
                .map(|config| config.tools.local_exec.max_output_bytes)
                .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
                .max(DEFAULT_MAX_OUTPUT_BYTES),
            stdin: Mutex::new(stdin),
            state: Mutex::new(ExecSessionState::default()),
            notify: Notify::new(),
        });
        self.sessions.insert(session_id, session.clone());

        if let Some(stdout) = stdout {
            spawn_reader(stdout, session.clone());
        }
        if let Some(stderr) = stderr {
            spawn_reader(stderr, session.clone());
        }
        spawn_waiter(child, session.clone());

        self.collect_output(session_id, request.yield_time_ms, request.max_output_tokens)
            .await
    }

    async fn write_stdin(
        &self,
        request: &WriteStdinRequest,
        context: &ToolContext,
    ) -> TalosResult<ExecResponse> {
        let session = self
            .sessions
            .get(&request.session_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| {
                TalosError::ToolError(format!("session {} not found", request.session_id))
            })?;

        if session.owner_session_id != context.session_id() {
            return Err(TalosError::ToolError(format!(
                "session {} does not belong to Talos session {}",
                request.session_id,
                context.session_id()
            )));
        }

        if !request.chars.is_empty() {
            let mut stdin_guard = session.stdin.lock().await;
            let stdin = stdin_guard.as_mut().ok_or_else(|| {
                TalosError::ToolError(format!(
                    "session {} is no longer writable",
                    request.session_id
                ))
            })?;
            stdin
                .write_all(request.chars.as_bytes())
                .await
                .map_err(|err| TalosError::ToolError(format!("failed to write stdin: {}", err)))?;
            stdin
                .flush()
                .await
                .map_err(|err| TalosError::ToolError(format!("failed to flush stdin: {}", err)))?;
        }

        self.collect_output(
            request.session_id,
            request.yield_time_ms,
            request.max_output_tokens,
        )
        .await
    }

    async fn collect_output(
        &self,
        session_id: i64,
        yield_time_ms: u64,
        max_output_tokens: Option<usize>,
    ) -> TalosResult<ExecResponse> {
        let session = self
            .sessions
            .get(&session_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| TalosError::ToolError(format!("session {} not found", session_id)))?;

        if yield_time_ms > 0 {
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(yield_time_ms)) => {}
                _ = session.notify.notified() => {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        let mut state = session.state.lock().await;
        let pending = &state.output[state.delivered_bytes..];
        let max_bytes = max_output_tokens.unwrap_or(session.max_output_bytes);
        let (chunk, consumed_bytes, truncated) = truncate_prefix(pending, max_bytes);
        state.delivered_bytes += consumed_bytes;

        let status = if state.exit_code.is_some() {
            ExecStatus::Exited
        } else {
            ExecStatus::Running
        };
        let exit_code = state.exit_code;
        let keep_session =
            status == ExecStatus::Running || state.delivered_bytes < state.output.len();
        drop(state);

        if !keep_session {
            self.sessions.remove(&session_id);
        }

        Ok(ExecResponse {
            session_id: if keep_session {
                Some(session.session_id)
            } else {
                None
            },
            status,
            output: chunk,
            exit_code,
            truncated,
        })
    }
}

pub async fn exec_command(
    request: &ExecCommandRequest,
    context: &ToolContext,
) -> TalosResult<ExecResponse> {
    ExecSessionManager::global()
        .open_session(request, context)
        .await
}

pub async fn write_stdin(request: &WriteStdinRequest) -> TalosResult<ExecResponse> {
    ExecSessionManager::global()
        .write_stdin(request, &ToolContext::default())
        .await
}

pub async fn write_stdin_with_context(
    request: &WriteStdinRequest,
    context: &ToolContext,
) -> TalosResult<ExecResponse> {
    ExecSessionManager::global()
        .write_stdin(request, context)
        .await
}

fn build_command(request: &ExecCommandRequest, workdir: &PathBuf) -> TalosResult<Command> {
    let shell = request
        .shell
        .clone()
        .or_else(|| std::env::var("SHELL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "sh".to_string());

    let mut command = Command::new(shell);
    command
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if request.login {
        command.arg("-lc");
    } else {
        command.arg("-c");
    }
    command.arg(&request.cmd);

    if request.tty {
        command.env("TERM", "xterm-256color");
    }

    Ok(command)
}

fn spawn_reader<R>(reader: R, session: Arc<ExecSession>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = reader;
        let mut buffer = vec![0_u8; 4096];
        loop {
            match reader.read(&mut buffer).await {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let chunk = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    let mut state = session.state.lock().await;
                    state.output.push_str(&chunk);
                    drop(state);
                    session.notify.notify_waiters();
                }
                Err(_) => {
                    session.notify.notify_waiters();
                    break;
                }
            }
        }
    });
}

fn spawn_waiter(mut child: Child, session: Arc<ExecSession>) {
    tokio::spawn(async move {
        let exit_code = child.wait().await.ok().and_then(|status| status.code());
        {
            let mut state = session.state.lock().await;
            state.exit_code = exit_code;
        }
        let mut stdin_guard = session.stdin.lock().await;
        stdin_guard.take();
        session.notify.notify_waiters();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exec_runtime_supports_sessionized_stdin() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let exec = exec_command(
            &ExecCommandRequest {
                cmd: "printf 'ready\\n'; read line; printf 'done:%s\\n' \"$line\"".to_string(),
                workdir: None,
                shell: Some("sh".to_string()),
                login: false,
                tty: false,
                yield_time_ms: 50,
                max_output_tokens: None,
                sandbox_permissions: SandboxPermissions::UseDefault,
                justification: None,
                prefix_rule: None,
            },
            &context,
        )
        .await
        .expect("exec should succeed");

        assert_eq!(exec.status, ExecStatus::Running);
        assert!(exec.output.contains("ready"));

        let session_id = exec.session_id.expect("session id should be present");
        let output = write_stdin_with_context(
            &WriteStdinRequest {
                session_id,
                chars: "ok\n".to_string(),
                yield_time_ms: 50,
                max_output_tokens: None,
            },
            &context,
        )
        .await
        .expect("write_stdin should succeed");

        assert!(output.output.contains("done:ok"));
        assert_eq!(output.status, ExecStatus::Exited);
    }

    #[tokio::test]
    async fn test_exec_runtime_allows_escalated_command_when_called_directly() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let result = exec_command(
            &ExecCommandRequest {
                cmd: "echo hi".to_string(),
                workdir: None,
                shell: Some("sh".to_string()),
                login: false,
                tty: false,
                yield_time_ms: 50,
                max_output_tokens: None,
                sandbox_permissions: SandboxPermissions::RequireEscalated,
                justification: Some("needs unrestricted filesystem access".to_string()),
                prefix_rule: None,
            },
            &context,
        )
        .await
        .expect("direct runtime execution no longer applies approval policy");

        assert!(result.output.contains("hi") || result.status == ExecStatus::Exited);
    }

    #[tokio::test]
    async fn test_pre_approved_context_bypasses_escalation_check() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"))
            .with_pre_approved();
        let result = exec_command(
            &ExecCommandRequest {
                cmd: "echo approved".to_string(),
                workdir: None,
                shell: Some("sh".to_string()),
                login: false,
                tty: false,
                yield_time_ms: 200,
                max_output_tokens: None,
                sandbox_permissions: SandboxPermissions::RequireEscalated,
                justification: Some("pre-approved via replay".to_string()),
                prefix_rule: None,
            },
            &context,
        )
        .await
        .expect("pre-approved context should bypass escalation check");

        assert!(result.output.contains("approved") || result.status == ExecStatus::Exited);
    }

    #[tokio::test]
    async fn test_write_stdin_rejects_cross_session_access() {
        let owner_context = crate::ToolContext::new("session-owner", "user-1")
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let foreign_context = crate::ToolContext::new("session-foreign", "user-2")
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let exec = exec_command(
            &ExecCommandRequest {
                cmd: "printf 'ready\\n'; sleep 1".to_string(),
                workdir: None,
                shell: Some("sh".to_string()),
                login: false,
                tty: false,
                yield_time_ms: 50,
                max_output_tokens: None,
                sandbox_permissions: SandboxPermissions::UseDefault,
                justification: None,
                prefix_rule: None,
            },
            &owner_context,
        )
        .await
        .expect("exec should succeed");

        let session_id = exec.session_id.expect("session id should be present");
        let error = write_stdin_with_context(
            &WriteStdinRequest {
                session_id,
                chars: "noop".to_string(),
                yield_time_ms: 0,
                max_output_tokens: None,
            },
            &foreign_context,
        )
        .await
        .expect_err("cross-session write_stdin should be rejected");

        assert!(error
            .to_string()
            .contains("does not belong to Talos session session-foreign"));
    }
}
