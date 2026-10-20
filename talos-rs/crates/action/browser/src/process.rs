//! Bounded process-group cleanup for normal completion, failed startup and dropped invocations.
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
    time::Duration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::process::Command;
use tokio_util::task::TaskTracker;

#[derive(Default)]
pub(crate) struct ProcessCleanup {
    tasks: TaskTracker,
    failure: Mutex<Option<TalosError>>,
}
impl ProcessCleanup {
    pub async fn drain(&self) -> TalosResult<()> {
        self.tasks.close();
        let _reopen = Reopen(&self.tasks);
        tokio::time::timeout(Duration::from_secs(5), self.tasks.wait())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "Browser process cleanup timed out.",
                    error,
                )
            })?;
        if let Some(error) = self
            .failure
            .lock()
            .map_err(|_| TalosError::internal("Browser cleanup state is poisoned."))?
            .as_ref()
        {
            return Err(error.clone());
        }
        Ok(())
    }
    fn record(&self, error: TalosError) {
        if let Ok(mut failure) = self.failure.lock() {
            failure.get_or_insert(error);
        }
    }
}
pub(crate) struct BrowserProcess {
    child: Option<Box<dyn ChildWrapper>>,
    cleanup: Arc<ProcessCleanup>,
}
impl BrowserProcess {
    pub fn spawn(command: Command, cleanup: Arc<ProcessCleanup>) -> TalosResult<Self> {
        let mut command = CommandWrap::from(command);
        command.wrap(KillOnDrop);
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        let child = command.spawn().map_err(|error| {
            TalosError::from_source(
                ErrorCode::ServiceUnavailable,
                "Failed to start a packaged browser process.",
                error,
            )
        })?;
        Ok(Self {
            child: Some(child),
            cleanup,
        })
    }
}
impl Deref for BrowserProcess {
    type Target = dyn ChildWrapper;
    fn deref(&self) -> &Self::Target {
        self.child.as_ref().expect("owned browser process").as_ref()
    }
}
impl DerefMut for BrowserProcess {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.child.as_mut().expect("owned browser process").as_mut()
    }
}
impl Drop for BrowserProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let killed = child.start_kill();
            let owner = self.cleanup.clone();
            if let Err(error) = killed {
                if error.raw_os_error() != Some(libc::ESRCH) {
                    owner.record(TalosError::from_source(
                        ErrorCode::RunCleanupUnconfirmed,
                        "Browser process group termination failed.",
                        error,
                    ));
                }
            }
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                self.cleanup.tasks.spawn_on(
                    async move {
                        match tokio::time::timeout(Duration::from_secs(4), child.wait()).await {
                            Ok(Ok(_)) => {}
                            Ok(Err(error)) => owner.record(TalosError::from_source(
                                ErrorCode::RunCleanupUnconfirmed,
                                "Browser process group exit could not be observed.",
                                error,
                            )),
                            Err(error) => owner.record(TalosError::from_source(
                                ErrorCode::RunCleanupUnconfirmed,
                                "Browser process group did not exit in time.",
                                error,
                            )),
                        }
                    },
                    &runtime,
                );
            } else {
                owner.record(TalosError::new(
                    ErrorCode::RunCleanupUnconfirmed,
                    "The browser cleanup executor is unavailable.",
                ));
            }
        }
    }
}

struct Reopen<'a>(&'a TaskTracker);
impl Drop for Reopen<'_> {
    fn drop(&mut self) {
        self.0.reopen();
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    #[tokio::test]
    async fn dropped_process_reaps_its_group_before_cleanup_completes() {
        let cleanup = Arc::new(ProcessCleanup::default());
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 60 & echo $!; wait"])
            .stdout(std::process::Stdio::piped());
        let mut child = BrowserProcess::spawn(command, cleanup.clone()).unwrap();
        let output = child.inner_mut().stdout().take().unwrap();
        let mut lines = BufReader::new(output).lines();
        let pid = lines
            .next_line()
            .await
            .unwrap()
            .unwrap()
            .parse::<i32>()
            .unwrap();
        drop(child);
        cleanup.drain().await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if unsafe { libc::kill(pid, 0) } == -1
                    && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}
