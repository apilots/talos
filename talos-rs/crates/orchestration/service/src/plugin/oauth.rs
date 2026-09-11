use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

const AUTHORIZATION_SECONDS: u64 = 600;
#[derive(Default)]
pub(super) struct OAuthAttempts {
    entries: Mutex<Vec<Arc<Attempt>>>,
    stopped: AtomicBool,
    pub tasks: TaskTracker,
}
struct Attempt {
    owner: McpDefinitionRef,
    definition: McpServerDefinition,
    config: McpOAuthConfig,
    id: String,
    expires_at: u64,
    cancel: CancellationToken,
    finished: CancellationToken,
    state: Mutex<McpAuthenticationState>,
}
impl OAuthAttempts {
    pub fn cancel(&self, owner: &McpDefinitionRef) {
        for entry in self
            .entries
            .lock()
            .expect("OAuth attempts lock poisoned")
            .iter()
            .filter(|entry| entry.owner == *owner)
        {
            entry.cancel.cancel();
        }
    }
    pub fn invalidate(&self, matches: impl Fn(&McpDefinitionRef, &McpServerDefinition) -> bool) {
        self.entries
            .lock()
            .expect("OAuth attempts lock poisoned")
            .retain(|entry| {
                if !matches(&entry.owner, &entry.definition) {
                    entry.cancel.cancel();
                }
                !entry.cancel.is_cancelled() || !entry.finished.is_cancelled()
            });
    }
    pub fn stop(&self) -> Vec<CancellationToken> {
        self.stopped.store(true, Ordering::Release);
        let finished = self
            .entries
            .lock()
            .expect("OAuth attempts lock poisoned")
            .drain(..)
            .map(|entry| {
                entry.cancel.cancel();
                entry.finished.clone()
            })
            .collect();
        self.tasks.close();
        finished
    }
    pub fn response(
        &self,
        owner: &McpDefinitionRef,
        definition: &McpServerDefinition,
    ) -> Option<McpAuthenticationResponse> {
        self.entries
            .lock()
            .expect("OAuth attempts lock poisoned")
            .iter()
            .find(|entry| {
                entry.owner == *owner
                    && !entry.cancel.is_cancelled()
                    && entry.definition == *definition
            })
            .map(|entry| {
                let mut state = entry
                    .state
                    .lock()
                    .expect("OAuth state lock poisoned")
                    .clone();
                if now() >= entry.expires_at
                    && matches!(state, McpAuthenticationState::Authorizing { .. })
                {
                    state = McpAuthenticationState::Expired;
                }
                McpAuthenticationResponse {
                    server: owner.clone(),
                    authentication: McpAuthentication::OAuth {
                        config: Box::new(entry.config.clone()),
                    },
                    state,
                }
            })
    }
}

impl crate::TalosService {
    /// Starts a local browser grant. The returned URL is private to the initiating caller.
    /// Callback exchange uses the SDK; only the current attempt may commit its credential bundle.
    pub async fn start_mcp_authentication(
        &self,
        request: McpAuthenticationStartRequest,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        let context = self.plugins.mcp_context()?;
        let (attempt, previous) = {
            let _guard = self.plugins.execution_gate.write().await;
            if context.oauth.stopped.load(Ordering::Acquire) {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "MCP is shutting down.",
                ));
            }
            let definition = self.plugins.definition(&request.server)?;
            if !matches!(
                definition.transport,
                McpTransportDefinition::StreamableHttp { .. }
            ) {
                return Err(TalosError::new(
                    ErrorCode::InvalidRequest,
                    "OAuth requires a Streamable HTTP MCP service.",
                ));
            }
            let previous = context
                .oauth
                .entries
                .lock()
                .expect("OAuth attempts lock poisoned")
                .iter()
                .filter(|entry| entry.owner == request.server && !entry.finished.is_cancelled())
                .map(|entry| entry.finished.clone())
                .collect::<Vec<_>>();
            context.oauth.cancel(&request.server);
            let mut entries = context
                .oauth
                .entries
                .lock()
                .expect("OAuth attempts lock poisoned");
            entries.retain(|entry| {
                if now() >= entry.expires_at {
                    entry.cancel.cancel();
                }
                !entry.cancel.is_cancelled() || !entry.finished.is_cancelled()
            });
            if entries.len() >= 32 {
                if let Some(index) = entries
                    .iter()
                    .position(|entry| entry.finished.is_cancelled())
                {
                    entries.remove(index);
                } else {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "Too many unfinished MCP authorization operations.",
                    ));
                }
            }
            let id = uuid::Uuid::new_v4().to_string();
            let expires_at = now() + AUTHORIZATION_SECONDS;
            let attempt = Arc::new(Attempt {
                owner: request.server.clone(),
                definition,
                config: request.config.clone(),
                id: id.clone(),
                expires_at,
                cancel: CancellationToken::new(),
                finished: CancellationToken::new(),
                state: Mutex::new(McpAuthenticationState::Authorizing {
                    attempt_id: id,
                    expires_at: expires_at as i64,
                }),
            });
            entries.push(attempt.clone());
            (attempt, previous)
        };
        let finished = attempt.finished.clone().drop_guard();
        let McpTransportDefinition::StreamableHttp { url, .. } = &attempt.definition.transport
        else {
            unreachable!()
        };
        let started = tokio::select! {
            _ = attempt.cancel.cancelled() => Err(cancelled()),
            result = tokio::time::timeout(Duration::from_secs(30), async {
                for previous in previous { previous.cancelled().await; }
                talos_mcp::OAuthAuthorization::start(url, &request.config, request.client_secret).await
            }) => result.map_err(|_| TalosError::new(ErrorCode::ToolExecutionFailed, "MCP OAuth discovery timed out.")).and_then(|result| result),
        };
        let authorization = match started {
            Ok(value) => value,
            Err(error) => {
                *attempt.state.lock().expect("OAuth state lock poisoned") =
                    McpAuthenticationState::Failed {
                        error: error.to_api_error(),
                    };
                return Err(error);
            }
        };
        let _guard = self.plugins.execution_gate.write().await;
        if attempt.cancel.is_cancelled()
            || self.plugins.definition(&attempt.owner)? != attempt.definition
        {
            return Err(cancelled());
        }
        let response = McpAuthenticationStartResponse {
            server: request.server,
            attempt_id: attempt.id.clone(),
            authorization_url: authorization.authorization_url().to_string(),
            expires_at: attempt.expires_at as i64,
        };
        let service = self.clone();
        context.oauth.tasks.spawn(async move {
            let _finished = finished;
            service
                .complete_mcp_authorization(attempt, authorization)
                .await;
        });
        Ok(response)
    }

    async fn complete_mcp_authorization(
        &self,
        attempt: Arc<Attempt>,
        authorization: talos_mcp::OAuthAuthorization,
    ) {
        let result = tokio::select! {
            _ = attempt.cancel.cancelled() => return,
            result = tokio::time::timeout(Duration::from_secs(attempt.expires_at.saturating_sub(now())), authorization.finish()) => result.map_err(|_| TalosError::new(ErrorCode::Unauthenticated, "MCP authorization expired.")).and_then(|result| result),
        };
        let _guard = self.plugins.execution_gate.write().await;
        if attempt.cancel.is_cancelled() {
            return;
        }
        let result = result.and_then(|credentials| {
            let context = self.plugins.mcp_context()?;
            let mut current = self.plugins.definition(&attempt.owner)?;
            if current != attempt.definition {
                return Err(cancelled());
            }
            super::credential_input::extract_inline(&mut current, &mut None)?;
            current.credentials = Some(context.credentials.create_oauth(
                &attempt.owner,
                &current,
                attempt.config.clone(),
                credentials,
            )?);
            self.publish_mcp_credentials(&attempt.owner, current)
        });
        if let Err(error) = result {
            talos_logging::report_error(&error);
            *attempt.state.lock().expect("OAuth state lock poisoned") =
                McpAuthenticationState::Failed {
                    error: error.to_api_error(),
                };
        } else if let Ok(context) = self.plugins.mcp_context() {
            context.oauth.cancel(&attempt.owner);
        }
        self.plugins.recover_credentials();
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn cancelled() -> TalosError {
    TalosError::new(
        ErrorCode::Cancelled,
        "The MCP authorization attempt is no longer current.",
    )
}
