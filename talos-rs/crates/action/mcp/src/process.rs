use crate::cleanup::Cleanup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::sync::Arc;
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::process::{ChildStdin, ChildStdout, Command};

/// Owns the process tree separately from the SDK transport so root exit is
/// observable even when a descendant keeps stdout open. Drop cleanup is tracked
/// by the application connection owner, including cancelled startup futures.
pub(super) struct McpProcess {
    child: Option<Box<dyn ChildWrapper>>,
    stderr: tokio::task::AbortHandle,
    cleanup: Arc<Cleanup>,
}

impl McpProcess {
    pub(super) fn spawn(
        command: Command,
        cleanup: &Arc<Cleanup>,
    ) -> TalosResult<((ChildStdout, ChildStdin), Self)> {
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        let mut child = command
            .spawn()
            .map_err(|error| process_error("Failed to start the MCP server process.", error))?;
        let stdout = child
            .inner_mut()
            .stdout()
            .take()
            .expect("MCP stdout is piped");
        let stdin = child
            .inner_mut()
            .stdin()
            .take()
            .expect("MCP stdin is piped");
        let mut stderr = child
            .inner_mut()
            .stderr()
            .take()
            .expect("MCP stderr is piped");
        let stderr = cleanup
            .tasks
            .spawn(async move {
                if let Err(error) = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await {
                    talos_logging::report_error(&process_error(
                        "MCP stderr could not be drained.",
                        error,
                    ));
                }
            })
            .abort_handle();
        Ok((
            (stdout, stdin),
            Self {
                child: Some(child),
                stderr,
                cleanup: cleanup.clone(),
            },
        ))
    }

    pub(super) async fn wait(&mut self) -> TalosResult<std::process::ExitStatus> {
        self.child
            .as_mut()
            .expect("MCP process is owned")
            .inner_mut()
            .wait()
            .await
            .map_err(|error| process_error("The MCP process exit could not be observed.", error))
    }

    pub(super) fn try_wait(&mut self) -> TalosResult<Option<std::process::ExitStatus>> {
        self.child
            .as_mut()
            .expect("MCP process is owned")
            .inner_mut()
            .try_wait()
            .map_err(|error| process_error("The MCP process exit could not be observed.", error))
    }

    pub(super) async fn shutdown(mut self) -> TalosResult<()> {
        // Keep the child in this Drop guard while awaiting. If the caller is
        // cancelled, the existing tracked Drop cleanup must still own the group.
        let result = cleanup(self.child.as_deref_mut().expect("MCP process is owned")).await;
        self.child.take();
        self.stderr.abort();
        result
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let stderr = self.stderr.clone();
            let cleanup_owner = self.cleanup.clone();
            self.cleanup.tasks.spawn(async move {
                if let Err(error) = cleanup(&mut *child).await {
                    cleanup_owner.record(error);
                }
                stderr.abort();
            });
        }
    }
}

async fn cleanup(child: &mut dyn ChildWrapper) -> TalosResult<()> {
    let deadline = tokio::time::Instant::now() + crate::CLEANUP_TIMEOUT;
    let kill_result = child.start_kill();
    // Darwin can reject signals during proc_exit before WNOHANG can reap the
    // root. Await the existing wrapper, then confirm the group signal again.
    // A genuine permission failure is never accepted as successful cleanup.
    #[cfg(target_os = "macos")]
    let kill_result = match kill_result {
        Err(error) if error.raw_os_error() == Some(libc::EPERM) => {
            tokio::time::timeout_at(deadline, child.wait())
                .await
                .map_err(|_| TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "The MCP process tree could not be terminated.",
                    error,
                ).context("The root did not become waitable before the cleanup deadline after a rejected group signal."))?
                .map_err(|error| TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "The MCP process exit could not be confirmed.",
                    error,
                ))?;
            child.start_kill()
        }
        result => result,
    };
    if let Err(error) = kill_result {
        #[cfg(unix)]
        let already_gone = error.raw_os_error() == Some(libc::ESRCH);
        #[cfg(not(unix))]
        let already_gone = false;
        if !already_gone {
            return Err(TalosError::from_source(
                ErrorCode::RunCleanupUnconfirmed,
                "The MCP process tree could not be terminated.",
                error,
            ));
        }
    }
    tokio::time::timeout_at(deadline, child.wait())
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "The MCP process did not exit within the cleanup deadline.",
            )
        })?
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::RunCleanupUnconfirmed,
                "The MCP process could not be reaped.",
                error,
            )
        })?;
    Ok(())
}

pub(super) fn process_error(message: &'static str, error: std::io::Error) -> TalosError {
    TalosError::from_source(ErrorCode::ToolExecutionFailed, message, error)
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
