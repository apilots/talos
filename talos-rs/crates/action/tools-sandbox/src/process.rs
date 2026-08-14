use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::{ExitStatus, Stdio};

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStderr, ChildStdout, Command};

use crate::facade::{TalosSandboxError, ToolSandboxEnforcement};
use crate::manager::PreparedToolCommand;
use crate::{
    SandboxStdin, ToolFileSystemHandle, ToolSandboxAuditEvent, ToolSandboxAuditOutcome,
    ToolSandboxAuditStage, ToolSandboxOperationKind, ToolSandboxPermit,
};

pub type ToolExecutionFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, TalosSandboxError>> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolProcessCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
    pub stdin: SandboxStdin,
}

impl ToolProcessCommand {
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: cwd.into(),
            stdin: SandboxStdin::Closed,
        }
    }
}

pub struct RunningToolProcess {
    child: Child,
    enforcement: ToolSandboxEnforcement,
    _private_tmp: Option<tempfile::TempDir>,
}

/// Identifies the OS process scope that the tool runtime may safely terminate.
///
/// The sandbox process layer owns this distinction because callers must not infer a process group
/// from a child PID. A process group is returned only after it is verified to be isolated from the
/// Talos process group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolProcessTerminationTarget {
    Process(u32),
    ProcessGroup(u32),
}

impl ToolProcessTerminationTarget {
    #[cfg(unix)]
    pub fn terminate(self) -> std::io::Result<()> {
        self.signal(libc::SIGTERM)
    }

    #[cfg(unix)]
    pub fn kill(self) -> std::io::Result<()> {
        self.signal(libc::SIGKILL)
    }

    #[cfg(unix)]
    fn signal(self, signal: libc::c_int) -> std::io::Result<()> {
        let target = match self {
            Self::Process(process_id) => process_id as libc::pid_t,
            Self::ProcessGroup(process_group_id) => -(process_group_id as libc::pid_t),
        };
        // SAFETY: `target` is derived from a live child PID and the signal is a valid libc signal.
        if unsafe { libc::kill(target, signal) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }
}

impl RunningToolProcess {
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn termination_target(&self) -> Option<ToolProcessTerminationTarget> {
        let process_id = self.id()?;

        #[cfg(unix)]
        {
            let process_id = process_id as libc::pid_t;
            // SAFETY: both calls only inspect process-group metadata for a live child/current process.
            let child_process_group = unsafe { libc::getpgid(process_id) };
            // SAFETY: `getpgrp` has no preconditions.
            let talos_process_group = unsafe { libc::getpgrp() };
            if child_process_group == process_id && child_process_group != talos_process_group {
                return Some(ToolProcessTerminationTarget::ProcessGroup(
                    process_id as u32,
                ));
            }
        }

        Some(ToolProcessTerminationTarget::Process(process_id))
    }

    pub fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    pub fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    pub async fn wait(&mut self) -> Result<ExitStatus, TalosSandboxError> {
        self.child
            .wait()
            .await
            .map_err(|error| TalosSandboxError::SandboxExecutionFailed {
                reason: error.to_string(),
            })
    }

    pub async fn kill(&mut self) -> Result<(), TalosSandboxError> {
        self.child
            .kill()
            .await
            .map_err(|error| TalosSandboxError::SandboxExecutionFailed {
                reason: error.to_string(),
            })
    }

    pub fn enforcement(&self) -> &ToolSandboxEnforcement {
        &self.enforcement
    }
}

impl ToolSandboxPermit {
    pub fn spawn(
        &self,
        command: ToolProcessCommand,
    ) -> ToolExecutionFuture<'_, RunningToolProcess> {
        Box::pin(async move {
            let binding = self.binding()?;
            self.require_operation(
                &binding.context.invocation,
                ToolSandboxOperationKind::SpawnProcess,
            )?;
            let prepared = binding
                .execution
                .manager()
                .prepare_command(command, &binding.context);
            let result = match prepared {
                Ok(prepared) => spawn_prepared(prepared).await,
                Err(error) => Err(error),
            };
            let (outcome, enforcement, error_category) = match &result {
                Ok(process) => (
                    ToolSandboxAuditOutcome::Succeeded,
                    Some(process.enforcement().clone()),
                    None,
                ),
                Err(error) => (
                    ToolSandboxAuditOutcome::Failed,
                    None,
                    Some(error.category()),
                ),
            };
            binding.execution.record(&ToolSandboxAuditEvent {
                invocation: binding.context.invocation.clone(),
                stage: ToolSandboxAuditStage::SpawnProcess,
                outcome,
                user_mode: binding.context.user_mode,
                effective_policy: Some(binding.context.effective_policy.clone()),
                enforcement,
                error_category,
            });
            result
        })
    }

    pub fn file_system(&self) -> Result<ToolFileSystemHandle, TalosSandboxError> {
        let binding = self.binding()?;
        self.require_operation(
            &binding.context.invocation,
            ToolSandboxOperationKind::FileSystem,
        )?;
        Ok(ToolFileSystemHandle::new(
            binding.context.clone(),
            binding.execution.clone(),
        ))
    }
}

async fn spawn_prepared(
    prepared: PreparedToolCommand,
) -> Result<RunningToolProcess, TalosSandboxError> {
    let PreparedToolCommand {
        command,
        enforcement,
        private_tmp,
    } = prepared;
    let mut process = Command::new(command.program().as_path());
    process
        .args(command.args())
        .env_clear()
        .envs(command.env())
        .current_dir(command.cwd().as_path())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(unix)]
    process.process_group(0);

    match command.stdin() {
        SandboxStdin::Closed => {
            process.stdin(Stdio::null());
        }
        SandboxStdin::Bytes(_) => {
            process.stdin(Stdio::piped());
        }
    }

    let mut child = process
        .spawn()
        .map_err(|error| TalosSandboxError::SandboxExecutionFailed {
            reason: error.to_string(),
        })?;
    if let SandboxStdin::Bytes(bytes) = command.stdin() {
        let mut stdin =
            child
                .stdin
                .take()
                .ok_or_else(|| TalosSandboxError::SandboxExecutionFailed {
                    reason: "sandboxed process stdin is unavailable".to_string(),
                })?;
        stdin.write_all(bytes).await.map_err(|error| {
            TalosSandboxError::SandboxExecutionFailed {
                reason: error.to_string(),
            }
        })?;
    }

    Ok(RunningToolProcess {
        child,
        enforcement,
        _private_tmp: private_tmp,
    })
}

impl Drop for RunningToolProcess {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
