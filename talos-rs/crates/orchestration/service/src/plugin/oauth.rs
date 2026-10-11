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

use agent_protocol::{
    McpAuthenticationCallbackParams, McpAuthenticationStartParams, McpOAuthCallbackOutcome,
    McpOAuthCallbackTarget,
};

const AUTHORIZATION_SECONDS: u64 = 600;
#[derive(Default)]
pub(super) struct OAuthAttempts {
    entries: Mutex<Vec<Arc<Attempt>>>,
    stopped: AtomicBool,
    pub tasks: TaskTracker,
}
struct RelayDelivery {
    callback_url: String,
    completion: tokio::sync::oneshot::Sender<TalosResult<()>>,
}
struct Attempt {
    relay: Option<(String, String, tokio::sync::mpsc::Sender<RelayDelivery>)>,
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

impl super::PluginManagerService {
    /// Starts a local browser grant. The returned URL is private to the initiating caller.
    /// Callback exchange uses the SDK; only the current attempt may commit its credential bundle.
    pub async fn start_mcp_authentication(
        &self,
        request: McpAuthenticationStartRequest,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        self.start_mcp_authentication_with_callback(
            McpAuthenticationStartParams {
                request,
                callback_target: McpOAuthCallbackTarget::Loopback,
            },
            /*initiator_user_id*/ None,
        )
        .await
    }

    pub async fn start_mcp_authentication_with_callback(
        &self,
        params: McpAuthenticationStartParams,
        initiator_user_id: Option<String>,
    ) -> TalosResult<McpAuthenticationStartResponse> {
        let request = params.request;
        let (relay, callbacks) = match params.callback_target {
            McpOAuthCallbackTarget::Loopback => (None, None),
            McpOAuthCallbackTarget::ControlPlaneRelay { uri } => {
                let (sender, receiver) = tokio::sync::mpsc::channel(1);
                let user_id = initiator_user_id
                    .filter(|id| !id.is_empty())
                    .ok_or_else(invalid_callback)?;
                (Some((uri, user_id, sender)), Some(receiver))
            }
        };
        let context = self.mcp_context()?;
        let (attempt, previous) = {
            let _guard = self.execution_gate.read().await;
            if context.oauth.stopped.load(Ordering::Acquire) {
                return Err(TalosError::new(
                    ErrorCode::ServiceUnavailable,
                    "MCP is shutting down.",
                ));
            }
            let definition = self.authorization_definition(&request.server)?;
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
                relay,
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
                match &attempt.relay {
                    Some((uri, _, _)) => talos_mcp::OAuthAuthorization::start_relay(url, &request.config, request.client_secret, uri).await,
                    None => talos_mcp::OAuthAuthorization::start(url, &request.config, request.client_secret).await,
                }
            }) => result.map_err(|_| TalosError::new(ErrorCode::ToolExecutionFailed, "MCP OAuth discovery timed out.")).and_then(|result| result),
        };
        if attempt.cancel.is_cancelled() {
            return Err(cancelled());
        }
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
        let _guard = self.execution_gate.read().await;
        if attempt.cancel.is_cancelled()
            || self.authorization_definition(&attempt.owner)? != attempt.definition
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
                .complete_mcp_authorization(attempt, authorization, callbacks)
                .await;
        });
        Ok(response)
    }

    async fn complete_mcp_authorization(
        &self,
        attempt: Arc<Attempt>,
        authorization: talos_mcp::OAuthAuthorization,
        callbacks: Option<tokio::sync::mpsc::Receiver<RelayDelivery>>,
    ) {
        let (result, completion, guard) = if let Some(mut callbacks) = callbacks {
            loop {
                let delivery = tokio::select! {
                    biased;
                    _ = attempt.cancel.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(attempt.expires_at.saturating_sub(now()))) => {
                        *attempt.state.lock().expect("OAuth state lock poisoned") = McpAuthenticationState::Expired;
                        return;
                    }
                    delivery = callbacks.recv() => match delivery { Some(delivery) => delivery, None => return },
                };
                let guard = match self.commit_guard() {
                    Ok(guard) => guard,
                    Err(error) => {
                        let _ = delivery.completion.send(Err(error));
                        continue;
                    }
                };
                let result = tokio::select! {
                    biased;
                    _ = attempt.cancel.cancelled() => return,
                    result = tokio::time::timeout(Duration::from_secs(attempt.expires_at.saturating_sub(now())), authorization.complete_callback(&delivery.callback_url)) => result.map_err(|_| TalosError::new(ErrorCode::ResourceExpired, "MCP authorization expired.")).and_then(|result|result),
                };
                if result
                    .as_ref()
                    .is_err_and(|error| error.error_code() == ErrorCode::InvalidRequest)
                {
                    let _ = delivery.completion.send(result.map(|_| ()));
                    continue;
                }
                break (result, Some(delivery.completion), Some(guard));
            }
        } else {
            let result = tokio::select! {
                biased;
                _ = attempt.cancel.cancelled() => return,
                result = tokio::time::timeout(Duration::from_secs(attempt.expires_at.saturating_sub(now())), authorization.finish()) => result.map_err(|_| TalosError::new(ErrorCode::ResourceExpired, "MCP authorization expired.")).and_then(|result|result),
            };
            let guard = self.commit_guard();
            match guard {
                Ok(guard) => (result, None, Some(guard)),
                Err(error) => (Err(error), None, None),
            }
        };
        let _guard = guard;
        if attempt.cancel.is_cancelled() {
            return;
        }
        let result = result.and_then(|credentials| {
            let context = self.mcp_context()?;
            let mut current = self.authorization_definition(&attempt.owner)?;
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
        if let Err(error) = &result {
            *attempt.state.lock().expect("OAuth state lock poisoned") =
                McpAuthenticationState::Failed {
                    error: error.to_api_error(),
                };
            if completion.is_none() {
                talos_logging::report_error(error);
            }
        } else if let Ok(context) = self.mcp_context() {
            context.oauth.cancel(&attempt.owner);
        }
        self.recover_credentials();
        if let Some(completion) = completion {
            let _ = completion.send(result);
        }
    }

    /// Delivers to the original live attempt once; neither a callback nor disconnect creates credentials.
    pub async fn complete_mcp_authentication(
        &self,
        user_id: &str,
        params: McpAuthenticationCallbackParams,
    ) -> TalosResult<McpAuthenticationResponse> {
        let context = self.mcp_context()?;
        let attempt = context
            .oauth
            .entries
            .lock()
            .expect("OAuth attempts lock poisoned")
            .iter()
            .find(|attempt| {
                attempt.owner == params.server
                    && attempt.id == params.attempt_id
                    && !attempt.cancel.is_cancelled()
            })
            .cloned()
            .ok_or_else(cancelled)?;
        if now() >= attempt.expires_at {
            return Err(TalosError::new(
                ErrorCode::ResourceExpired,
                "MCP authorization expired.",
            ));
        }
        let (uri, initiator, sender) = attempt.relay.as_ref().ok_or_else(invalid_callback)?;
        if initiator != user_id
            || self.authorization_definition(&attempt.owner)? != attempt.definition
        {
            return Err(cancelled());
        }
        let mut callback = url::Url::parse(uri).map_err(|_| invalid_callback())?;
        callback
            .query_pairs_mut()
            .append_pair("state", &params.state);
        if let Some(issuer) = &params.issuer {
            callback.query_pairs_mut().append_pair("iss", issuer);
        }
        match params.outcome {
            McpOAuthCallbackOutcome::AuthorizationCode(code) => {
                callback.query_pairs_mut().append_pair("code", &code);
            }
            McpOAuthCallbackOutcome::ProviderErrorCode(code) => {
                callback.query_pairs_mut().append_pair("error", &code);
            }
        }
        let (completion, result) = tokio::sync::oneshot::channel();
        sender
            .try_send(RelayDelivery {
                callback_url: callback.into(),
                completion,
            })
            .map_err(|error| match error {
                tokio::sync::mpsc::error::TrySendError::Full(_) => {
                    TalosError::new(ErrorCode::Conflict, "An MCP callback is already pending.")
                }
                tokio::sync::mpsc::error::TrySendError::Closed(_) => cancelled(),
            })?;
        result.await.map_err(|_| cancelled())??;
        self.read_mcp_authentication(McpAuthenticationReadRequest {
            node_id: None,
            server: params.server,
        })
    }

    /// Called by the host when its registration is revoked or its outbound connection is lost.
    pub fn cancel_remote_mcp_authorizations(&self) {
        if let Some(context) = &self.mcp {
            for attempt in context
                .oauth
                .entries
                .lock()
                .expect("OAuth attempts lock poisoned")
                .iter()
                .filter(|attempt| attempt.relay.is_some())
            {
                attempt.cancel.cancel();
            }
        }
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

fn invalid_callback() -> TalosError {
    TalosError::new(
        ErrorCode::InvalidRequest,
        "The MCP authorization callback is invalid.",
    )
}

/// Builds the trusted fixed callback route from the configured product control-plane origin.
pub fn mcp_relay_callback_uri(control_plane: &str) -> TalosResult<String> {
    let value = format!(
        "{}/v1/mcp/oauth/callback",
        control_plane.trim_end_matches('/')
    );
    let uri = url::Url::parse(&value).map_err(|_| invalid_callback())?;
    if uri.scheme() != "https"
        || !uri.username().is_empty()
        || uri.password().is_some()
        || uri.query().is_some()
        || uri.fragment().is_some()
    {
        return Err(invalid_callback());
    }
    Ok(uri.into())
}
