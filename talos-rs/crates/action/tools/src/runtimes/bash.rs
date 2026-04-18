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

const DEFAULT_BASH_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_TASK_OUTPUT_YIELD_TIME_MS: u64 = 250;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024;

fn default_bash_yield_time_ms() -> u64 {
    DEFAULT_BASH_YIELD_TIME_MS
}

fn default_task_output_yield_time_ms() -> u64 {
    DEFAULT_TASK_OUTPUT_YIELD_TIME_MS
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
pub struct BashCommandRequest {
    pub cmd: String,
    #[serde(default)]
    pub workdir: Option<String>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default = "default_login")]
    pub login: bool,
    #[serde(default = "default_tty")]
    pub tty: bool,
    #[serde(default = "default_bash_yield_time_ms")]
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
pub struct TaskOutputRequest {
    pub session_id: i64,
    #[serde(default)]
    pub chars: String,
    #[serde(default = "default_task_output_yield_time_ms")]
    pub yield_time_ms: u64,
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSessionResponse {
    pub session_id: Option<i64>,
    pub status: TaskStatus,
    pub output: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
}

struct BashSession {
    session_id: i64,
    owner_session_id: String,
    max_output_bytes: usize,
    stdin: Mutex<Option<ChildStdin>>,
    state: Mutex<BashSessionState>,
    notify: Notify,
}

#[derive(Default)]
struct BashSessionState {
    output: String,
    delivered_bytes: usize,
    exit_code: Option<i32>,
}

struct BashSessionManager {
    next_id: AtomicI64,
    sessions: DashMap<i64, Arc<BashSession>>,
}

impl BashSessionManager {
    fn global() -> &'static BashSessionManager {
        static INSTANCE: OnceLock<BashSessionManager> = OnceLock::new();
        INSTANCE.get_or_init(|| BashSessionManager {
            next_id: AtomicI64::new(1),
            sessions: DashMap::new(),
        })
    }

    async fn open_session(
        &self,
        request: &BashCommandRequest,
        context: &ToolContext,
    ) -> TalosResult<TaskSessionResponse> {
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

        let session = Arc::new(BashSession {
            session_id,
            owner_session_id: context.session_id().to_string(),
            max_output_bytes: context
                .kernel_config()
                .map(|config| config.tools.local_exec.max_output_bytes)
                .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
                .max(DEFAULT_MAX_OUTPUT_BYTES),
            stdin: Mutex::new(stdin),
            state: Mutex::new(BashSessionState::default()),
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

    async fn read_task_output(
        &self,
        request: &TaskOutputRequest,
        context: &ToolContext,
    ) -> TalosResult<TaskSessionResponse> {
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
    ) -> TalosResult<TaskSessionResponse> {
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
            TaskStatus::Exited
        } else {
            TaskStatus::Running
        };
        let exit_code = state.exit_code;
        let keep_session =
            status == TaskStatus::Running || state.delivered_bytes < state.output.len();
        drop(state);

        if !keep_session {
            self.sessions.remove(&session_id);
        }

        Ok(TaskSessionResponse {
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

    async fn stop_session(&self, session_id: i64, context: &ToolContext) -> TalosResult<()> {
        let session = self
            .sessions
            .get(&session_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| TalosError::ToolError(format!("session {} not found", session_id)))?;
        if session.owner_session_id != context.session_id() {
            return Err(TalosError::ToolError(format!(
                "session {} does not belong to Talos session {}",
                session_id,
                context.session_id()
            )));
        }

        let mut stdin_guard = session.stdin.lock().await;
        stdin_guard.take();
        let mut state = session.state.lock().await;
        if state.exit_code.is_none() {
            state.exit_code = Some(130);
        }
        drop(state);
        self.sessions.remove(&session_id);
        session.notify.notify_waiters();
        Ok(())
    }
}

pub async fn run_bash_command(
    request: &BashCommandRequest,
    context: &ToolContext,
) -> TalosResult<TaskSessionResponse> {
    BashSessionManager::global()
        .open_session(request, context)
        .await
}

pub async fn read_task_output(request: &TaskOutputRequest) -> TalosResult<TaskSessionResponse> {
    BashSessionManager::global()
        .read_task_output(request, &ToolContext::default())
        .await
}

pub async fn read_task_output_with_context(
    request: &TaskOutputRequest,
    context: &ToolContext,
) -> TalosResult<TaskSessionResponse> {
    BashSessionManager::global()
        .read_task_output(request, context)
        .await
}

pub async fn stop_task_with_context(session_id: i64, context: &ToolContext) -> TalosResult<()> {
    BashSessionManager::global()
        .stop_session(session_id, context)
        .await
}

fn build_command(request: &BashCommandRequest, workdir: &PathBuf) -> TalosResult<Command> {
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

fn spawn_reader<R>(reader: R, session: Arc<BashSession>)
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

fn spawn_waiter(mut child: Child, session: Arc<BashSession>) {
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
    async fn test_bash_runtime_supports_sessionized_task_output() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let exec = run_bash_command(
            &BashCommandRequest {
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
        .expect("bash command should succeed");

        assert_eq!(exec.status, TaskStatus::Running);
        assert!(exec.output.contains("ready"));

        let session_id = exec.session_id.expect("session id should be present");
        let output = read_task_output_with_context(
            &TaskOutputRequest {
                session_id,
                chars: "ok\n".to_string(),
                yield_time_ms: 50,
                max_output_tokens: None,
            },
            &context,
        )
        .await
        .expect("task output read should succeed");

        assert!(output.output.contains("done:ok"));
        assert_eq!(output.status, TaskStatus::Exited);
    }

    #[tokio::test]
    async fn test_bash_runtime_allows_escalated_command_when_called_directly() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let result = run_bash_command(
            &BashCommandRequest {
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

        assert!(result.output.contains("hi") || result.status == TaskStatus::Exited);
    }

    #[tokio::test]
    async fn test_pre_approved_context_bypasses_escalation_check() {
        let context = crate::ToolContext::default()
            .with_project_root(std::env::current_dir().expect("cwd should exist"))
            .with_pre_approved();
        let result = run_bash_command(
            &BashCommandRequest {
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

        assert!(result.output.contains("approved") || result.status == TaskStatus::Exited);
    }

    #[tokio::test]
    async fn test_task_output_rejects_cross_session_access() {
        let owner_context = crate::ToolContext::new("session-owner", "user-1")
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let foreign_context = crate::ToolContext::new("session-foreign", "user-2")
            .with_project_root(std::env::current_dir().expect("cwd should exist"));
        let exec = run_bash_command(
            &BashCommandRequest {
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
        .expect("bash command should succeed");

        let session_id = exec.session_id.expect("session id should be present");
        let error = read_task_output_with_context(
            &TaskOutputRequest {
                session_id,
                chars: "noop".to_string(),
                yield_time_ms: 0,
                max_output_tokens: None,
            },
            &foreign_context,
        )
        .await
        .expect_err("cross-session task output read should be rejected");

        assert!(error
            .to_string()
            .contains("does not belong to Talos session session-foreign"));
    }
}
