//! Invocation ownership and safe drain of the existing Bash Session tasks.
use super::*;

/// Returning a running/completed response transfers ownership back to the
/// manager. Dropping the invocation future must instead reclaim that Session.
pub(super) struct InvocationGuard<'a> {
    manager: &'a BashSessionManager,
    session: Arc<BashSession>,
    waiter: tokio::task::AbortHandle,
    timeout: Option<tokio::task::AbortHandle>,
    armed: bool,
}

impl<'a> InvocationGuard<'a> {
    pub fn new(
        manager: &'a BashSessionManager,
        session: Arc<BashSession>,
        waiter: tokio::task::AbortHandle,
        timeout: Option<tokio::task::AbortHandle>,
    ) -> Self {
        Self {
            manager,
            session,
            waiter,
            timeout,
            armed: true,
        }
    }

    pub fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for InvocationGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        self.manager.sessions.remove(&self.session.session_id);
        self.session.abandoned.store(true, Ordering::Release);
        self.session.reader_stop.cancel();
        if let Some(timeout) = &self.timeout {
            timeout.abort();
        }
        #[cfg(unix)]
        if !self.waiter.is_finished() {
            if let Some(target) = self.session.termination_target {
                if let Err(error) = target.kill() {
                    if !self.waiter.is_finished() {
                        talos_logging::report_error(&TalosError::internal_with_source(
                            "Forced Bash cleanup could not terminate its process scope.",
                            error,
                        ));
                    }
                }
            }
        }
        // Dropping RunningToolProcess also requests child termination on every
        // platform. Readers remain alive until accepted file writes and their
        // last-reader finalization finish; no detached writer is introduced.
        self.waiter.abort();
    }
}

pub(super) async fn join_session_tasks(session: &BashSession) {
    let Some(tasks) = session.tasks.lock().await.take() else {
        return;
    };
    if let Some(timeout) = tasks.timeout {
        if session.state.lock().await.termination == Some(BashTermination::TimedOut) {
            // The timeout owner has claimed termination but may still be in
            // its TERM/KILL grace period. Do not cancel that cleanup halfway.
            let _ = timeout.await;
        } else {
            timeout.abort();
            let _ = timeout.await;
        }
    }
    if !join_or_abort(tasks.waiter).await {
        session.output.lock().await.interrupted = true;
    }
    for mut reader in tasks.readers {
        let result = match tokio::time::timeout(
            Duration::from_millis(TASK_JOIN_GRACE_MS),
            &mut reader,
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                // Cancel only pipe reads. A spill/backfill/append that already
                // owns bytes must complete before its writer can be finalized.
                session.reader_stop.cancel();
                let result = reader.await;
                session.output.lock().await.interrupted = true;
                result
            }
        };
        if let Err(failure) = result {
            let mut capture = session.output.lock().await;
            capture.interrupted = true;
            let error =
                TalosError::internal_with_source("Bash output reader task failed.", failure);
            capture.record_error(error);
        }
    }
}

async fn join_or_abort(mut task: JoinHandle<()>) -> bool {
    if tokio::time::timeout(Duration::from_millis(TASK_JOIN_GRACE_MS), &mut task)
        .await
        .is_err()
    {
        task.abort();
        let _ = task.await;
        return false;
    }
    true
}
