use std::path::Path;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncRead, AsyncReadExt};
#[cfg(not(unix))]
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};

use crate::runtimes::{
    map_sandbox_process_error, process_environment, resolve_base_dir, resolve_executable,
    resolve_path, truncate_prefix,
};
use crate::{ToolContext, ToolProgressEmitter, ToolProgressKind};
use talos_tools_sandbox::{
    RunningToolProcess, SandboxStdin, ToolProcessCommand, ToolProcessTerminationTarget,
};

const DEFAULT_BASH_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024;
const TERMINAL_OUTPUT_DRAIN_GRACE_MS: u64 = 100;
const PROCESS_TERMINATE_GRACE_MS: u64 = 200;

fn default_bash_yield_time_ms() -> u64 {
    DEFAULT_BASH_YIELD_TIME_MS
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
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<usize>,
    #[serde(default)]
    pub sandbox_permissions: SandboxPermissions,
    #[serde(default)]
    pub justification: Option<String>,
    #[serde(default)]
    pub prefix_rule: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BashRunStatus {
    Running,
    TimedOut,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BashCommandKind {
    ReadOnly,
    BuildOrTest,
    FilesystemWrite,
    Network,
    ProcessControl,
    Destructive,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BashCommandRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashSessionResponse {
    pub session_id: Option<i64>,
    pub status: BashRunStatus,
    pub output: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub truncated: bool,
    pub elapsed_ms: u64,
    #[serde(default)]
    pub output_offset: usize,
    #[serde(default)]
    pub next_offset: usize,
    #[serde(default)]
    pub command_kind: Option<BashCommandKind>,
    #[serde(default)]
    pub risk: Option<BashCommandRisk>,
}

struct BashSession {
    session_id: i64,
    max_output_bytes: usize,
    termination_target: Option<ToolProcessTerminationTarget>,
    state: Mutex<BashSessionState>,
    notify: Notify,
    started_at: Instant,
    command_kind: BashCommandKind,
    risk: BashCommandRisk,
    progress_emitter: Option<ToolProgressEmitter>,
}

#[derive(Default)]
struct BashSessionState {
    stdout: String,
    stderr: String,
    output: String,
    delivered_bytes: usize,
    exit_code: Option<i32>,
    timed_out: bool,
}

#[derive(Debug, Clone, Copy)]
enum OutputStreamKind {
    Stdout,
    Stderr,
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
    ) -> TalosResult<BashSessionResponse> {
        if request.cmd.trim().is_empty() {
            return Err(TalosError::public(
                talos_error::ErrorCode::ToolExecutionFailed,
                "cmd cannot be empty".to_string(),
            ));
        }

        let base_dir = resolve_base_dir(context)?;
        let workdir = request
            .workdir
            .as_deref()
            .map(|path| resolve_path(&base_dir, path))
            .unwrap_or(base_dir);

        let command = build_command(request, &workdir)?;
        let mut child = context
            .sandbox_permit()?
            .spawn(command)
            .await
            .map_err(map_sandbox_process_error)?;

        let session_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let termination_target = child.termination_target();
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let (command_kind, risk) = classify_command(&request.cmd);

        let session = Arc::new(BashSession {
            session_id,
            max_output_bytes: context
                .kernel_config()
                .map(|config| config.tools.local_exec.max_output_bytes)
                .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
                .max(DEFAULT_MAX_OUTPUT_BYTES),
            termination_target,
            state: Mutex::new(BashSessionState::default()),
            notify: Notify::new(),
            started_at: Instant::now(),
            command_kind,
            risk,
            progress_emitter: context.progress_emitter(),
        });
        self.sessions.insert(session_id, session.clone());

        if let Some(stdout) = stdout {
            spawn_reader(stdout, session.clone(), OutputStreamKind::Stdout);
        }
        if let Some(stderr) = stderr {
            spawn_reader(stderr, session.clone(), OutputStreamKind::Stderr);
        }
        spawn_waiter(child, session.clone());
        if let Some(timeout_ms) = request.timeout_ms {
            spawn_timeout(session.clone(), timeout_ms);
        }

        self.collect_output(
            session_id,
            request.yield_time_ms,
            None,
            request.max_output_tokens,
        )
        .await
    }

    async fn collect_output(
        &self,
        session_id: i64,
        yield_time_ms: u64,
        offset: Option<usize>,
        max_output_tokens: Option<usize>,
    ) -> TalosResult<BashSessionResponse> {
        let session = self
            .sessions
            .get(&session_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| {
                TalosError::public(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("session {} not found", session_id),
                )
            })?;

        if yield_time_ms > 0 {
            let deadline = Instant::now() + Duration::from_millis(yield_time_ms);
            loop {
                {
                    let state = session.state.lock().await;
                    if state.exit_code.is_some() || state.timed_out {
                        drop(state);
                        tokio::time::sleep(Duration::from_millis(TERMINAL_OUTPUT_DRAIN_GRACE_MS))
                            .await;
                        break;
                    }
                }

                let now = Instant::now();
                if now >= deadline {
                    break;
                }

                tokio::select! {
                    _ = tokio::time::sleep(deadline.saturating_duration_since(now)) => break,
                    _ = session.notify.notified() => {}
                }
            }
        }

        let mut state = session.state.lock().await;
        let start = offset
            .unwrap_or(state.delivered_bytes)
            .min(state.output.len());
        let pending = &state.output[start..];
        let max_bytes = max_output_tokens.unwrap_or(session.max_output_bytes);
        let (chunk, consumed_bytes, truncated) = truncate_prefix(pending, max_bytes);
        state.delivered_bytes = state.delivered_bytes.max(start + consumed_bytes);

        let status = if state.timed_out {
            BashRunStatus::TimedOut
        } else if state.exit_code.is_some() {
            BashRunStatus::Exited
        } else {
            BashRunStatus::Running
        };
        let exit_code = state.exit_code;
        let stdout = state.stdout.clone();
        let stderr = state.stderr.clone();
        let keep_session = status == BashRunStatus::Running;
        let next_offset = state.delivered_bytes;
        drop(state);

        if !keep_session {
            self.sessions.remove(&session_id);
        }

        Ok(BashSessionResponse {
            session_id: if keep_session {
                Some(session.session_id)
            } else {
                None
            },
            status,
            output: chunk,
            stdout,
            stderr,
            exit_code,
            truncated,
            elapsed_ms: elapsed_ms(&session),
            output_offset: start,
            next_offset,
            command_kind: Some(session.command_kind.clone()),
            risk: Some(session.risk.clone()),
        })
    }
}

pub async fn run_bash_command(
    request: &BashCommandRequest,
    context: &ToolContext,
) -> TalosResult<BashSessionResponse> {
    BashSessionManager::global()
        .open_session(request, context)
        .await
}

fn build_command(request: &BashCommandRequest, workdir: &Path) -> TalosResult<ToolProcessCommand> {
    let shell = request
        .shell
        .clone()
        .or_else(|| std::env::var("SHELL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "sh".to_string());

    let mut args = Vec::new();

    if request.login {
        args.push("-lc".to_string());
    } else {
        args.push("-c".to_string());
    }
    args.push(request.cmd.clone());

    let mut env = process_environment();
    if request.tty {
        env.insert("TERM".to_string(), "xterm-256color".to_string());
    }

    Ok(ToolProcessCommand {
        program: resolve_executable(&shell)?,
        args,
        env,
        cwd: workdir.to_path_buf(),
        stdin: SandboxStdin::Closed,
    })
}

fn spawn_reader<R>(reader: R, session: Arc<BashSession>, stream_kind: OutputStreamKind)
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
                    match stream_kind {
                        OutputStreamKind::Stdout => state.stdout.push_str(&chunk),
                        OutputStreamKind::Stderr => state.stderr.push_str(&chunk),
                    }
                    state.output.push_str(&chunk);
                    drop(state);
                    if let Some(emitter) = &session.progress_emitter {
                        emitter.emit(crate::ToolProgressEvent {
                            kind: match stream_kind {
                                OutputStreamKind::Stdout => ToolProgressKind::Stdout,
                                OutputStreamKind::Stderr => ToolProgressKind::Stderr,
                            },
                            text: chunk,
                        });
                    }
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

fn elapsed_ms(session: &BashSession) -> u64 {
    session
        .started_at
        .elapsed()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn classify_command(command: &str) -> (BashCommandKind, BashCommandRisk) {
    let facts = crate::command_risk::classify_bash_command(command);
    let risk = crate::command_risk::display_bash_risk(facts.risk);
    (facts.kind, risk)
}

fn spawn_waiter(mut child: RunningToolProcess, session: Arc<BashSession>) {
    tokio::spawn(async move {
        let exit_code = child.wait().await.ok().and_then(|status| status.code());
        {
            let mut state = session.state.lock().await;
            if !state.timed_out {
                state.exit_code = exit_code;
            }
        }
        session.notify.notify_waiters();
    });
}

fn spawn_timeout(session: Arc<BashSession>, timeout_ms: u64) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
        {
            let state = session.state.lock().await;
            if state.exit_code.is_some() || state.timed_out {
                return;
            }
        }
        if let Some(termination_target) = session.termination_target {
            terminate_process(termination_target).await;
        }
        {
            let mut state = session.state.lock().await;
            if state.exit_code.is_none() && !state.timed_out {
                state.timed_out = true;
                state.exit_code = Some(124);
            }
        }
        session.notify.notify_waiters();
    });
}

#[cfg(unix)]
async fn terminate_process(termination_target: ToolProcessTerminationTarget) {
    let _ = termination_target.terminate();
    tokio::time::sleep(Duration::from_millis(PROCESS_TERMINATE_GRACE_MS)).await;
    let _ = termination_target.kill();
}

#[cfg(not(unix))]
async fn terminate_process(termination_target: ToolProcessTerminationTarget) {
    let ToolProcessTerminationTarget::Process(process_id) = termination_target else {
        return;
    };
    let _ = Command::new("kill")
        .arg("-TERM")
        .arg(process_id.to_string())
        .status()
        .await;
    tokio::time::sleep(Duration::from_millis(PROCESS_TERMINATE_GRACE_MS)).await;
}

#[cfg(test)]
#[path = "bash_tests.rs"]
mod tests;
