//! Auth-domain verified contexts. Local readers never initialize or refresh them.
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use sha2::{Digest, Sha256};
use talos_cloud_sdk::{talos_cloud_types::IntrospectionRequestDto, TalosCloudClient};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::{sync::watch, task::JoinHandle, time::Instant};

use crate::{CloudSessionCache, RequestIdentity};

const REFRESH: Duration = Duration::from_secs(60);
const VALIDITY: Duration = Duration::from_secs(120);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONTEXTS: usize = 256;

type Fingerprint = [u8; 32];

#[derive(Default)]
pub(crate) struct AuthContexts {
    sessions: Mutex<HashMap<Fingerprint, Arc<Context>>>,
    closed: AtomicBool,
    tasks: tokio_util::task::TaskTracker,
}

struct Context {
    admission: tokio::sync::Mutex<()>,
    initialized: AtomicBool,
    state: watch::Sender<TalosResult<VerifiedContext>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

#[derive(Clone)]
struct VerifiedContext {
    identity: RequestIdentity,
    verified_at: Instant,
    expires_at_ms: Option<i64>,
}

impl VerifiedContext {
    fn identity(&self) -> TalosResult<RequestIdentity> {
        if self
            .expires_at_ms
            .is_some_and(|expiry| expiry <= CloudSessionCache::now_ms())
        {
            return Err(unauthenticated());
        }
        if self.verified_at.elapsed() >= VALIDITY {
            return Err(TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Authentication verification is temporarily unavailable.",
            ));
        }
        Ok(self.identity.clone())
    }
}

impl Context {
    fn stop(&self) {
        self.initialized.store(true, Ordering::SeqCst);
        let _ = self.state.send_replace(Err(unauthenticated()));
        if let Some(task) = self.task.lock().expect("Auth task lock").take() {
            task.abort();
        }
    }

    fn identity(&self) -> TalosResult<RequestIdentity> {
        self.state
            .borrow()
            .as_ref()
            .map_err(Clone::clone)?
            .identity()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(task) = self.task.get_mut().expect("Auth task lock").take() {
            task.abort();
        }
    }
}

impl AuthContexts {
    pub(crate) async fn shutdown(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.tasks.close();
        for context in self
            .sessions
            .lock()
            .expect("Auth contexts lock")
            .drain()
            .map(|(_, context)| context)
        {
            context.stop();
        }
        self.tasks.wait().await;
    }

    pub(crate) fn identity(&self, token: &str) -> TalosResult<RequestIdentity> {
        let sessions = self.sessions.lock().expect("Auth contexts lock");
        sessions
            .get(&fingerprint(token))
            .ok_or_else(unauthenticated)?
            .identity()
    }

    pub(crate) fn invalidate(&self, token: &str) {
        if let Some(context) = self
            .sessions
            .lock()
            .expect("Auth contexts lock")
            .remove(&fingerprint(token))
        {
            context.stop();
        }
    }

    pub(crate) async fn verify(
        &self,
        client: &TalosCloudClient,
        token: &str,
    ) -> TalosResult<RequestIdentity> {
        let key = fingerprint(token);
        let context = {
            let mut sessions = self.sessions.lock().expect("Auth contexts lock");
            if self.closed.load(Ordering::SeqCst) {
                return Err(unauthenticated());
            }
            sessions.retain(|_, context| {
                !context.initialized.load(Ordering::SeqCst) || context.identity().is_ok()
            });
            if let Some(context) = sessions.get(&key) {
                context.clone()
            } else {
                if sessions.len() >= MAX_CONTEXTS {
                    return Err(TalosError::new(
                        ErrorCode::ServiceUnavailable,
                        "Authentication context capacity reached.",
                    ));
                }
                let (state, _) = watch::channel(Err(unauthenticated()));
                let context = Arc::new(Context {
                    admission: Default::default(),
                    initialized: AtomicBool::new(false),
                    state,
                    task: Mutex::new(None),
                });
                sessions.insert(key, context.clone());
                context
            }
        };
        // Single flight per credential; unrelated login/reconnect cannot queue
        // behind a held issuer request. Local readers never take this lock.
        let _admission = context.admission.lock().await;
        if context.initialized.load(Ordering::SeqCst) {
            return context.identity();
        }
        let mut pending = PendingAdmission {
            contexts: self,
            key,
            context: &context,
            committed: false,
        };
        let verified = verify_remote(client, token).await;
        // Logout removes this generation before awaiting Cloud. A late result cannot publish.
        let mut sessions = self.sessions.lock().expect("Auth contexts lock");
        let current = sessions
            .get(&key)
            .is_some_and(|entry| Arc::ptr_eq(entry, &context));
        if !current {
            return Err(unauthenticated());
        }
        match verified {
            Ok(verified) => {
                let identity = verified.identity()?;
                let _ = context.state.send_replace(Ok(verified));
                let client = client.clone();
                let token = token.to_owned();
                let state = context.state.clone();
                // Stable credential jitter prevents aligned per-account refresh bursts.
                let refresh = REFRESH.mul_f64(0.9 + f64::from(key[0]) / 2550.0 * 2.0);
                let mut refresh = tokio::time::interval_at(Instant::now() + refresh, refresh);
                refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                let lifetime = RefreshLifetime;
                tracing::info!("Cloud authorization refresh started");
                *context.task.lock().expect("Auth task lock") =
                    Some(self.tasks.spawn(async move {
                        let _lifetime = lifetime;
                        loop {
                            refresh.tick().await;
                            match verify_remote(&client, &token).await {
                                Ok(verified) => {
                                    let same_account = state.borrow().as_ref().is_ok_and(|old| {
                                        old.identity.user_id == verified.identity.user_id
                                    });
                                    if !same_account {
                                        let _ = state.send_replace(Err(unauthenticated()));
                                        return;
                                    }
                                    let _ = state.send_replace(Ok(verified));
                                }
                                Err(error) => {
                                    let rejected = matches!(
                                        error.error_code(),
                                        ErrorCode::Unauthenticated | ErrorCode::PermissionDenied
                                    );
                                    talos_logging::report_error(&error);
                                    if rejected {
                                        let _ = state.send_replace(Err(error));
                                        return;
                                    }
                                    // Transient failure never extends the last verified deadline.
                                }
                            }
                        }
                    }));
                context.initialized.store(true, Ordering::SeqCst);
                pending.committed = true;
                Ok(identity)
            }
            Err(error) => {
                let _ = context.state.send_replace(Err(error.clone()));
                context.initialized.store(true, Ordering::SeqCst);
                sessions.remove(&key);
                Err(error)
            }
        }
    }
}

struct PendingAdmission<'a> {
    contexts: &'a AuthContexts,
    key: Fingerprint,
    context: &'a Arc<Context>,
    committed: bool,
}
impl Drop for PendingAdmission<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.context.stop();
            let mut sessions = self.contexts.sessions.lock().expect("Auth contexts lock");
            if sessions
                .get(&self.key)
                .is_some_and(|entry| Arc::ptr_eq(entry, self.context))
            {
                sessions.remove(&self.key);
            }
        }
    }
}

struct RefreshLifetime;
impl Drop for RefreshLifetime {
    fn drop(&mut self) {
        tracing::info!("Cloud authorization refresh stopped");
    }
}

async fn verify_remote(client: &TalosCloudClient, token: &str) -> TalosResult<VerifiedContext> {
    let response = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client.introspect(IntrospectionRequestDto {
            token: token.to_owned(),
        }),
    )
    .await
    .map_err(|error| {
        TalosError::from_source(
            ErrorCode::ServiceUnavailable,
            "Authentication verification timed out.",
            error,
        )
    })?
    .map_err(super::cloud::map_cloud_error)?;
    let expires_at_ms = response.expires_at_ms;
    let identity = super::cloud::map_identity(response, token)?;
    Ok(VerifiedContext {
        identity,
        verified_at: Instant::now(),
        expires_at_ms,
    })
}

fn fingerprint(token: &str) -> Fingerprint {
    Sha256::digest(token.as_bytes()).into()
}
fn unauthenticated() -> TalosError {
    TalosError::new(
        ErrorCode::Unauthenticated,
        "No valid local authentication context.",
    )
}

#[cfg(test)]
#[path = "context_tests.rs"]
mod tests;
