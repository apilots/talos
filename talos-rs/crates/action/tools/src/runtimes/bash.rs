use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use talos_error::{TalosError, TalosResult};
use tokio::io::{AsyncRead, AsyncReadExt};
#[cfg(not(unix))]
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};
use tokio::task::JoinHandle;

use crate::runtimes::{
    map_sandbox_error, process_environment, resolve_base_dir, resolve_executable, resolve_path,
};
use crate::{ToolContext, ToolProgressEmitter, ToolProgressKind};
use talos_tools_sandbox::{
    RunningToolProcess, SandboxStdin, ToolProcessCommand, ToolProcessTerminationTarget,
};

mod lifecycle;
use lifecycle::{join_session_tasks, InvocationGuard};

mod output;
mod output_log;
use output::OutputCapture;
pub use output_log::{maintain_bash_output_workspace, OutputLogMaintenanceStats};

const DEFAULT_BASH_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_MAX_OUTPUT_BYTES: usize = 16 * 1024;
const PROCESS_TERMINATE_GRACE_MS: u64 = 200;
const TASK_JOIN_GRACE_MS: u64 = 200;

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
    pub max_output_bytes: Option<usize>,
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
    Cancelled,
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
    pub output_log: Option<talos_types::talos::BashOutputLog>,
    pub capture_error: Option<talos_error::ApiError>,
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
    termination_target: Option<ToolProcessTerminationTarget>,
    state: Mutex<BashSessionState>,
    output: Mutex<OutputCapture>,
    notify: Notify,
    started_at: Instant,
    command_kind: BashCommandKind,
    risk: BashCommandRisk,
    progress_emitter: Option<ToolProgressEmitter>,
    cancellation: Option<crate::InvocationCancellation>,
    reader_stop: tokio_util::sync::CancellationToken,
    readers_remaining: AtomicUsize,
    abandoned: AtomicBool,
    tasks: Mutex<Option<BashSessionTasks>>,
}

struct BashSessionState {
    exit_code: Option<i32>,
    termination: Option<BashTermination>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BashTermination {
    TimedOut,
    Cancelled,
}

struct BashSessionTasks {
    readers: Vec<JoinHandle<()>>,
    waiter: JoinHandle<()>,
    timeout: Option<JoinHandle<()>>,
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
        policy: crate::spec::ToolResultPolicy,
    ) -> TalosResult<BashSessionResponse> {
        if request.cmd.trim().is_empty() {
            return Err(TalosError::new(
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

        let capture = OutputCapture::new(context, policy).await?;
        let command = build_command(request, &workdir)?;
        let mut child = context
            .sandbox_permit()?
            .spawn(command)
            .await
            .map_err(map_sandbox_error)?;

        let session_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let termination_target = child.termination_target();
        let stdout = child.take_stdout();
        let stderr = child.take_stderr();
        let (command_kind, risk) = classify_command(&request.cmd);

        let session = Arc::new(BashSession {
            session_id,
            termination_target,
            state: Mutex::new(BashSessionState {
                exit_code: None,
                termination: None,
            }),
            output: Mutex::new(capture),
            notify: Notify::new(),
            started_at: Instant::now(),
            command_kind,
            risk,
            progress_emitter: context.progress_emitter(),
            cancellation: context.cancellation(),
            reader_stop: tokio_util::sync::CancellationToken::new(),
            readers_remaining: AtomicUsize::new(
                usize::from(stdout.is_some()) + usize::from(stderr.is_some()),
            ),
            abandoned: AtomicBool::new(false),
            tasks: Mutex::new(None),
        });
        self.sessions.insert(session_id, session.clone());

        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            readers.push(spawn_reader(
                stdout,
                session.clone(),
                OutputStreamKind::Stdout,
            ));
        }
        if let Some(stderr) = stderr {
            readers.push(spawn_reader(
                stderr,
                session.clone(),
                OutputStreamKind::Stderr,
            ));
        }
        let waiter = spawn_waiter(child, session.clone());
        let timeout = request
            .timeout_ms
            .map(|timeout_ms| spawn_timeout(session.clone(), timeout_ms));
        let mut invocation = InvocationGuard::new(
            self,
            session.clone(),
            waiter.abort_handle(),
            timeout.as_ref().map(JoinHandle::abort_handle),
        );
        session.tasks.lock().await.replace(BashSessionTasks {
            readers,
            waiter,
            timeout,
        });

        let response = self
            .collect_output(session_id, request.yield_time_ms)
            .await?;
        invocation.disarm();
        Ok(response)
    }

    async fn collect_output(
        &self,
        session_id: i64,
        yield_time_ms: u64,
    ) -> TalosResult<BashSessionResponse> {
        let session = self
            .sessions
            .get(&session_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| {
                TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    format!("session {} not found", session_id),
                )
            })?;

        if yield_time_ms > 0 || session.cancellation.is_some() {
            let deadline = session
                .cancellation
                .is_none()
                .then(|| Instant::now() + Duration::from_millis(yield_time_ms));
            loop {
                {
                    let state = session.state.lock().await;
                    if state.exit_code.is_some() || state.termination.is_some() {
                        break;
                    }
                }

                let deadline_wait = async {
                    match deadline {
                        Some(deadline) => {
                            let now = Instant::now();
                            if now < deadline {
                                tokio::time::sleep(deadline.saturating_duration_since(now)).await;
                            }
                        }
                        None => std::future::pending::<()>().await,
                    }
                };

                tokio::select! {
                    _ = deadline_wait => break,
                    _ = session.notify.notified() => {}
                    () = wait_for_cancellation(session.cancellation.clone()) => {
                        cancel_session(&session).await;
                        break;
                    }
                }
            }
        }

        let terminal = {
            let state = session.state.lock().await;
            state.exit_code.is_some() || state.termination.is_some()
        };
        if terminal {
            join_session_tasks(&session).await;
        }

        let capture = session.output.lock().await;
        let (chunk, stdout, stderr) = capture.snapshot();
        let truncated = capture.truncated;
        let next_offset = capture.total_bytes as usize;
        let start = next_offset.saturating_sub(chunk.len());
        let output_log = capture.log.clone();
        let capture_error = capture.error.as_ref().map(TalosError::to_api_error);
        drop(capture);
        let state = session.state.lock().await;
        let status = match state.termination {
            Some(BashTermination::TimedOut) => BashRunStatus::TimedOut,
            Some(BashTermination::Cancelled) => BashRunStatus::Cancelled,
            None if state.exit_code.is_some() => BashRunStatus::Exited,
            None => BashRunStatus::Running,
        };
        let exit_code = state.exit_code;
        let keep_session = status == BashRunStatus::Running;
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
            output_log,
            capture_error,
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
    let max_inline_bytes = request.max_output_bytes.unwrap_or_else(|| {
        context
            .kernel_config()
            .map(|config| config.tools.bash.max_output_bytes)
            .unwrap_or(DEFAULT_MAX_OUTPUT_BYTES)
    });
    run_bash_command_with_policy(
        request,
        context,
        crate::spec::ToolResultPolicy {
            max_inline_bytes,
            persist_large_output: false,
            preview_lines: usize::MAX,
        },
    )
    .await
}

pub(crate) async fn run_bash_command_with_policy(
    request: &BashCommandRequest,
    context: &ToolContext,
    policy: crate::spec::ToolResultPolicy,
) -> TalosResult<BashSessionResponse> {
    BashSessionManager::global()
        .open_session(request, context, policy)
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

fn spawn_reader<R>(
    reader: R,
    session: Arc<BashSession>,
    stream_kind: OutputStreamKind,
) -> JoinHandle<()>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = reader;
        let mut buffer = vec![0_u8; 4096];
        let mut utf8_pending = Vec::new();
        loop {
            let read = tokio::select! {
                biased;
                () = session.reader_stop.cancelled() => break,
                read = reader.read(&mut buffer) => read,
            };
            match read {
                Ok(0) => break,
                Ok(bytes_read) => {
                    let mut capture = session.output.lock().await;
                    capture.append(&buffer[..bytes_read], stream_kind).await;
                    let remaining = capture
                        .policy_limit()
                        .saturating_sub(capture.progress_bytes);
                    let decoded = output::decode_progress(&mut utf8_pending, &buffer[..bytes_read]);
                    let text_budget = remaining.saturating_sub(128);
                    let text = crate::output_budget::truncate_text(&decoded, text_budget);
                    capture.progress_bytes += text.len();
                    let omission = if decoded.len() > text_budget && !capture.progress_omitted {
                        capture.progress_omitted = true;
                        let status = crate::output_budget::truncate_text(
                            "Output preview limit reached; see the final result for saved output.",
                            remaining.saturating_sub(text.len()),
                        );
                        capture.progress_bytes += status.len();
                        Some(status)
                    } else {
                        None
                    };
                    drop(capture);
                    if !text.is_empty() {
                        if let Some(emitter) = &session.progress_emitter {
                            emitter.emit(crate::ToolProgressEvent {
                                kind: match stream_kind {
                                    OutputStreamKind::Stdout => ToolProgressKind::Stdout,
                                    OutputStreamKind::Stderr => ToolProgressKind::Stderr,
                                },
                                text,
                            });
                        }
                    }
                    if let (Some(text), Some(emitter)) = (omission, &session.progress_emitter) {
                        if !text.is_empty() {
                            emitter.emit(crate::ToolProgressEvent {
                                kind: ToolProgressKind::Status,
                                text,
                            });
                        }
                    }
                    session.notify.notify_waiters();
                }
                Err(error) => {
                    let mut capture = session.output.lock().await;
                    capture.interrupted = true;
                    capture.record_error(TalosError::internal_with_source(
                        "Bash output pipe read failed.",
                        error,
                    ));
                    session.notify.notify_waiters();
                    break;
                }
            }
        }
        if session.readers_remaining.fetch_sub(1, Ordering::AcqRel) == 1 {
            let mut capture = session.output.lock().await;
            capture.interrupted |=
                session.abandoned.load(Ordering::Acquire) || session.reader_stop.is_cancelled();
            capture.finish().await;
            if let Some(error) = &capture.error {
                talos_logging::report_error(error);
            }
            session.notify.notify_waiters();
        }
    })
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

fn spawn_waiter(mut child: RunningToolProcess, session: Arc<BashSession>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let exit_code = child.wait().await.ok().and_then(|status| status.code());
        {
            let mut state = session.state.lock().await;
            if state.termination.is_none() {
                state.exit_code = exit_code;
            }
        }
        session.notify.notify_waiters();
    })
}

fn spawn_timeout(session: Arc<BashSession>, timeout_ms: u64) -> JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
        if !claim_termination(&session, BashTermination::TimedOut, 124).await {
            return;
        }
        if let Some(termination_target) = session.termination_target {
            terminate_process(termination_target).await;
        }
        session.notify.notify_waiters();
    })
}

async fn wait_for_cancellation(cancellation: Option<crate::InvocationCancellation>) {
    match cancellation {
        Some(cancellation) => cancellation.requested().await,
        None => std::future::pending().await,
    }
}

async fn cancel_session(session: &BashSession) {
    if !claim_termination(session, BashTermination::Cancelled, 130).await {
        return;
    }
    if let Some(termination_target) = session.termination_target {
        terminate_process(termination_target).await;
    }
    session.notify.notify_waiters();
}

async fn claim_termination(
    session: &BashSession,
    termination: BashTermination,
    exit_code: i32,
) -> bool {
    let mut state = session.state.lock().await;
    if state.exit_code.is_some() || state.termination.is_some() {
        return false;
    }
    state.termination = Some(termination);
    state.exit_code = Some(exit_code);
    true
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
