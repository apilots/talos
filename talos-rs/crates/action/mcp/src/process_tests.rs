use super::*;
use std::io;
use std::{future::Future, pin::Pin, process::ExitStatus};

// Model Darwin's observed boundary: killpg returns EPERM while proc_exit is in
// progress, but WNOHANG still reports no waitable root. The real child stays
// behind stdin EOF until the cleanup owner awaits it; no sleeps or message matching.
#[derive(Debug)]
struct ExitingGroup {
    child: tokio::process::Child,
    reaped: bool,
    after_reap: i32,
    wait_started: Option<tokio::sync::oneshot::Sender<()>>,
}
impl ChildWrapper for ExitingGroup {
    fn inner(&self) -> &dyn ChildWrapper {
        &self.child
    }
    fn inner_mut(&mut self) -> &mut dyn ChildWrapper {
        &mut self.child
    }
    fn into_inner(self: Box<Self>) -> Box<dyn ChildWrapper> {
        Box::new(self.child)
    }
    fn start_kill(&mut self) -> io::Result<()> {
        if self.reaped {
            Err(io::Error::from_raw_os_error(self.after_reap))
        } else if cfg!(target_os = "macos") {
            Err(io::Error::from_raw_os_error(libc::EPERM))
        } else {
            Ok(())
        }
    }
    fn wait(&mut self) -> Pin<Box<dyn Future<Output = io::Result<ExitStatus>> + Send + '_>> {
        Box::pin(async {
            if let Some(started) = self.wait_started.take() {
                started.send(()).unwrap();
                std::future::pending::<()>().await;
            }
            self.child.stdin.take();
            let status = self.child.wait().await?;
            self.reaped = true;
            Ok(status)
        })
    }
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn permission_during_exit_waits_for_root_before_confirming_group_cleanup() {
    let child = Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    cleanup(&mut ExitingGroup {
        child,
        reaped: false,
        after_reap: libc::ESRCH,
        wait_started: None,
    })
    .await
    .unwrap();
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn cleanup_reaps_an_exited_root_before_confirming_its_process_group_is_gone() {
    let cleanup_owner = Arc::new(Cleanup::default());
    let mut command = Command::new("/usr/bin/true");
    command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let (pipes, process) = McpProcess::spawn(command, &cleanup_owner).unwrap();
    let pid = process.child.as_ref().unwrap().id().unwrap();
    // Observe the real kernel exit without reaping it. macOS killpg returns
    // EPERM for this zombie-only group until its parent consumes the exit.
    let mut status: libc::siginfo_t = unsafe { std::mem::zeroed() };
    assert_eq!(
        unsafe { libc::waitid(libc::P_PID, pid, &mut status, libc::WEXITED | libc::WNOWAIT) },
        0
    );
    drop(pipes);
    process.shutdown().await.unwrap();
    cleanup_owner.tasks.close();
    cleanup_owner.tasks.wait().await;
    cleanup_owner.result().unwrap();
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn permission_denial_that_survives_reaping_is_reported_with_its_original_source() {
    use std::error::Error;
    let child = Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let error = cleanup(&mut ExitingGroup {
        child,
        reaped: false,
        after_reap: libc::EPERM,
        wait_started: None,
    })
    .await
    .unwrap_err();
    assert_eq!(error.error_code(), ErrorCode::RunCleanupUnconfirmed);
    assert_eq!(
        error
            .source()
            .unwrap()
            .downcast_ref::<io::Error>()
            .unwrap()
            .raw_os_error(),
        Some(libc::EPERM)
    );
}

#[tokio::test]
async fn cancelling_explicit_shutdown_transfers_the_child_and_stderr_to_tracked_cleanup() {
    let cleanup_owner = Arc::new(Cleanup::default());
    let child = Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let pid = child.id().unwrap();
    let stderr = cleanup_owner
        .tasks
        .spawn(std::future::pending::<()>())
        .abort_handle();
    let (started, observed) = tokio::sync::oneshot::channel();
    let process = McpProcess {
        child: Some(Box::new(ExitingGroup {
            child,
            reaped: false,
            after_reap: libc::ESRCH,
            wait_started: Some(started),
        })),
        stderr,
        cleanup: cleanup_owner.clone(),
    };
    let mut shutdown = Box::pin(process.shutdown());
    tokio::select! {
        entered = observed => entered.unwrap(),
        result = &mut shutdown => panic!("shutdown completed before cancellation barrier: {result:?}"),
    }
    drop(shutdown);
    cleanup_owner.tasks.close();
    tokio::time::timeout(crate::CLEANUP_TIMEOUT, cleanup_owner.tasks.wait())
        .await
        .unwrap();
    cleanup_owner.result().unwrap();
    assert!(cleanup_owner.tasks.is_empty());
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
    let owner = Arc::downgrade(&cleanup_owner);
    drop(cleanup_owner);
    assert!(owner.upgrade().is_none());
}

#[tokio::test]
async fn explicit_and_unpublished_drop_cleanup_reap_real_groups_and_drain_owned_tasks() {
    use tokio::io::{AsyncBufReadExt, BufReader};
    for explicit in [true, false] {
        let cleanup_owner = Arc::new(Cleanup::default());
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "sleep 300 & echo $!; wait"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let ((stdout, stdin), process) = McpProcess::spawn(command, &cleanup_owner).unwrap();
        let root = process.child.as_ref().unwrap().id().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let descendant: i32 = line.trim().parse().unwrap();
        if explicit {
            process.shutdown().await.unwrap();
        } else {
            drop(process);
        }
        drop((reader, stdin));
        cleanup_owner.tasks.close();
        tokio::time::timeout(crate::CLEANUP_TIMEOUT, cleanup_owner.tasks.wait())
            .await
            .unwrap();
        cleanup_owner.result().unwrap();
        assert!(cleanup_owner.tasks.is_empty());
        // The root is reaped by process-wrap. Orphan descendants are reaped by
        // the OS; verify disappearance rather than assuming a delivered signal.
        tokio::time::timeout(crate::CLEANUP_TIMEOUT, async {
            for pid in [root as i32, descendant] {
                loop {
                    if unsafe { libc::kill(pid, 0) } == -1 {
                        assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        })
        .await
        .unwrap();
        let owner = Arc::downgrade(&cleanup_owner);
        drop(cleanup_owner);
        assert!(owner.upgrade().is_none());
    }
}
