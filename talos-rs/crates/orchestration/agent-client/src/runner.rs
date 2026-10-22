use std::sync::Arc;
use std::time::Duration;

use agent_protocol::{
    Deployment, HeartbeatParams, NodeAuth, NodeHealth, NodeRequest, RegisterParams, Registration,
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use tokio::time::{interval_at, sleep, sleep_until, Instant};
use tokio_util::sync::CancellationToken;

use crate::AgentNodeClient;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const RENEW_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug)]
enum NodeOperation {
    Register,
    Heartbeat,
    RenewLease,
    ExecutionConnection,
    Unregister,
}

/// Independent node registration lifecycle. It never persists a Turn or
/// fabricates Harness capabilities. A caller must stop execution admission
/// when this runner ends with an expired or rejected lease.
pub struct AgentNodeRunner {
    client: AgentNodeClient,
    node_id: String,
    credential: String,
    deployments: Vec<Deployment>,
    system_info: Option<talos_types::NodeSystemInfo>,
    lease_observer: Option<AgentNodeLeaseObserver>,
    execution_adapter: Option<talos_agent::AgentHarnessRpcAdapter>,
}

pub type AgentNodeLeaseObserver =
    Arc<dyn Fn(Option<AgentNodeLeaseUpdate>) -> TalosResult<()> + Send + Sync>;

#[derive(Clone)]
pub struct AgentNodeLeaseUpdate {
    pub auth: NodeAuth,
    pub owner_user_id: String,
    pub validity: agent_protocol::LeaseValidity,
    pub deadline: Instant,
}

impl AgentNodeRunner {
    pub fn new(
        client: AgentNodeClient,
        node_id: String,
        credential: String,
        deployments: Vec<Deployment>,
    ) -> Self {
        Self {
            client,
            node_id,
            credential,
            deployments,
            system_info: None,
            lease_observer: None,
            execution_adapter: None,
        }
    }

    pub fn with_system_info(mut self, info: talos_types::NodeSystemInfo) -> Self {
        self.system_info = Some(info);
        self
    }

    pub fn with_lease_observer(mut self, observer: AgentNodeLeaseObserver) -> Self {
        self.lease_observer = Some(observer);
        self
    }

    /// Enable outbound execution through this node's existing Harness adapter.
    /// Its lease observer must install every published lease before Connect starts.
    pub fn with_execution_adapter(mut self, adapter: talos_agent::AgentHarnessRpcAdapter) -> Self {
        self.execution_adapter = Some(adapter);
        self
    }

    pub async fn run(self, shutdown: CancellationToken) -> TalosResult<()> {
        let result = self.run_registered(&shutdown).await;
        // Every exit, including an explicit authentication rejection, revokes
        // admission and live streams before the host starts graceful shutdown.
        let cleanup = self.publish_lease(None);
        cleanup?;
        if let Some(auth) = result? {
            let started = Instant::now();
            let unregister = tokio::time::timeout(
                Duration::from_secs(10),
                self.client.unregister(NodeRequest { auth, params: () }),
            )
            .await
            .unwrap_or_else(|error| {
                Err(TalosError::from_source(
                    ErrorCode::Timeout,
                    "Agent node unregister timed out",
                    error,
                ))
            });
            if let Err(error) = unregister {
                self.report_failure(NodeOperation::Unregister, started, &error);
            }
        }
        Ok(())
    }

    async fn run_registered(&self, shutdown: &CancellationToken) -> TalosResult<Option<NodeAuth>> {
        let mut connection: Option<ExecutionConnectionTask> = None;
        'registered: loop {
            if let Some(mut previous) = connection.take() {
                previous.stop.cancel();
                let _ = previous.wait().await?;
            }
            if let Some(adapter) = &self.execution_adapter {
                adapter.drain_disconnected().await?;
            }
            let Some(mut lease) = self.register_until_success(shutdown).await? else {
                return Ok(None);
            };
            if shutdown.is_cancelled() {
                return Ok(Some(lease.auth));
            }
            self.publish_lease(Some(lease.clone()))?;
            let connection_started = Instant::now();
            connection = self.execution_adapter.as_ref().map(|adapter| {
                let stop = shutdown.child_token();
                let task_stop = stop.clone();
                let client = self.client.clone();
                let auth = lease.auth.clone();
                let adapter = adapter.clone();
                let handle = tokio::spawn(async move {
                    client
                        .run_execution_connection(auth, adapter, task_stop)
                        .await
                });
                ExecutionConnectionTask {
                    stop,
                    handle: Some(handle),
                }
            });
            tracing::info!(node_id = %self.node_id, "Agent node registered");
            let mut heartbeat =
                interval_at(Instant::now() + HEARTBEAT_INTERVAL, HEARTBEAT_INTERVAL);
            let mut renewal = interval_at(Instant::now() + RENEW_INTERVAL, RENEW_INTERVAL);
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => {
                        return Ok((Instant::now() < lease.deadline).then_some(lease.auth));
                    }
                    result = async {
                        match &mut connection { Some(connection) => connection.wait().await, None => futures_util::future::pending().await }
                    } => {
                        if let Err(error) = result? {
                            if !is_transient(&error) { return Err(error); }
                            self.report_failure(NodeOperation::ExecutionConnection, connection_started, &error);
                        }
                        self.publish_lease(None)?;
                        tokio::select! { _ = shutdown.cancelled() => return Ok(Some(lease.auth)), _ = sleep(RETRY_INTERVAL) => {} }
                        continue 'registered;
                    }
                    _ = sleep_until(lease.deadline) => {
                        self.publish_lease(None)?;
                        tracing::warn!(node_id = %self.node_id, "Agent node lease expired locally; registration will be renewed");
                        continue 'registered;
                    }
                    _ = heartbeat.tick() => {
                        let sent_at = Instant::now();
                        let result = tokio::select! {
                            biased;
                            _ = shutdown.cancelled() => return Ok((Instant::now() < lease.deadline).then_some(lease.auth.clone())),
                            result = tokio::time::timeout_at(lease.deadline, self.client.heartbeat(NodeRequest {
                                auth: lease.auth.clone(),
                                params: HeartbeatParams { health: NodeHealth::Healthy },
                            })) => result.unwrap_or_else(|_| Err(TalosError::new(ErrorCode::ResourceExpired, "Agent node lease expired locally"))),
                        };
                        match result {
                            Ok(result) if result.registration_required => {
                                self.publish_lease(None)?;
                                continue 'registered
                            },
                            Ok(_) => {}
                            Err(error) if error.is_code(ErrorCode::ResourceExpired) => {
                                self.publish_lease(None)?;
                                continue 'registered
                            },
                            Err(error) if is_transient(&error) => {
                                self.report_failure(NodeOperation::Heartbeat, sent_at, &error);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    _ = renewal.tick() => {
                        let sent_at = Instant::now();
                        let result = tokio::select! {
                            biased;
                            _ = shutdown.cancelled() => return Ok((Instant::now() < lease.deadline).then_some(lease.auth.clone())),
                            result = tokio::time::timeout_at(lease.deadline, self.client.renew_lease(NodeRequest {
                                auth: lease.auth.clone(),
                                params: (),
                            })) => result.unwrap_or_else(|_| Err(TalosError::new(ErrorCode::ResourceExpired, "Agent node lease expired locally"))),
                        };
                        match result {
                            Ok(validity) => {
                                lease.deadline = local_deadline(sent_at, validity.expires_at, validity.server_at)?;
                                lease.validity = validity;
                                self.publish_lease(Some(lease.clone()))?;
                            },
                            Err(error) if error.is_code(ErrorCode::ResourceExpired) => {
                                self.publish_lease(None)?;
                                continue 'registered
                            },
                            Err(error) if is_transient(&error) => {
                                self.report_failure(NodeOperation::RenewLease, sent_at, &error);
                            }
                            Err(error) => return Err(error),
                        }
                    }
                }
            }
        }
    }

    fn publish_lease(&self, update: Option<AgentNodeLeaseUpdate>) -> TalosResult<()> {
        match &self.lease_observer {
            Some(observer) => observer(update),
            None => Ok(()),
        }
    }

    // Transient failures are consumed by this retry owner. Report the original
    // cause here; terminal errors still propagate to the existing host owner.
    fn report_failure(&self, operation: NodeOperation, started: Instant, error: &TalosError) {
        let span = tracing::warn_span!("agent_node_operation_failed", node_id = %self.node_id, ?operation, elapsed_ms = started.elapsed().as_millis() as u64);
        let _entered = span.enter();
        talos_logging::report_error(error);
    }

    async fn register_until_success(
        &self,
        shutdown: &CancellationToken,
    ) -> TalosResult<Option<AgentNodeLeaseUpdate>> {
        loop {
            let sent_at = Instant::now();
            if shutdown.is_cancelled() {
                return Ok(None);
            }
            // An issued Register can already have committed at the control plane.
            // Drain its bounded response so shutdown can unregister the returned lease.
            let registration = self
                .client
                .register(NodeRequest {
                    auth: NodeAuth {
                        node_id: self.node_id.clone(),
                        token: self.credential.clone(),
                    },
                    params: RegisterParams {
                        system_info: self.system_info.clone(),
                        deployments: self.deployments.clone(),
                    },
                })
                .await;
            match registration {
                Ok(registration) => {
                    return Ok(Some(active_lease(&self.node_id, registration, sent_at)?))
                }
                Err(error) if is_transient(&error) => {
                    self.report_failure(NodeOperation::Register, sent_at, &error);
                    tokio::select! {
                        _ = shutdown.cancelled() => return Ok(None),
                        _ = sleep(RETRY_INTERVAL) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn active_lease(
    node_id: &str,
    registration: Registration,
    sent_at: Instant,
) -> TalosResult<AgentNodeLeaseUpdate> {
    let expires_at = local_deadline(
        sent_at,
        registration.validity.expires_at,
        registration.validity.server_at,
    )?;
    let auth = NodeAuth {
        node_id: node_id.to_string(),
        token: registration.lease_token,
    };
    Ok(AgentNodeLeaseUpdate {
        auth,
        owner_user_id: registration.owner_user_id,
        validity: registration.validity,
        deadline: expires_at,
    })
}

fn local_deadline(sent_at: Instant, expires_at: i64, server_at: i64) -> TalosResult<Instant> {
    let lifetime = expires_at
        .checked_sub(server_at)
        .filter(|seconds| *seconds > 0)
        .ok_or_else(|| TalosError::internal("Agent node lease validity is malformed"))?;
    Ok(sent_at + Duration::from_secs(lifetime as u64))
}

fn is_transient(error: &TalosError) -> bool {
    error.is_code(ErrorCode::ServiceUnavailable)
        || error.is_code(ErrorCode::Timeout)
        || error.is_code(ErrorCode::Cancelled)
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;

struct ExecutionConnectionTask {
    stop: CancellationToken,
    handle: Option<tokio::task::JoinHandle<TalosResult<()>>>,
}
impl ExecutionConnectionTask {
    async fn wait(&mut self) -> TalosResult<TalosResult<()>> {
        let Some(handle) = self.handle.as_mut() else {
            return Ok(Ok(()));
        };
        let result = handle.await.map_err(|error| {
            TalosError::internal_with_source(
                "Agent connection task failed; technical details were withheld.",
                error,
            )
        })?;
        self.handle.take();
        Ok(result)
    }
}
impl Drop for ExecutionConnectionTask {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
