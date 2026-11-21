//! Account-scoped control-plane observations, refreshed outside metadata queries.
use std::{
    collections::HashMap,
    sync::{RwLock, Weak},
    time::{Duration, Instant},
};

use futures_util::{stream, StreamExt};
use talos_error::{ErrorCode, TalosResult};
use talos_service::Actor;
use talos_types::{ComputerListRequest, NodeHealth, NodeInfo, NodeStatus};

use super::application_sync::ApplicationSyncHost;

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_AGE: Duration = Duration::from_secs(15);
const RPC_TIMEOUT: Duration = Duration::from_secs(5);
const CONCURRENCY: usize = 4;

pub(crate) struct ComputerObservations {
    accounts: RwLock<HashMap<String, HashMap<String, Observation>>>,
    slots: tokio::sync::Semaphore,
}

impl Default for ComputerObservations {
    fn default() -> Self {
        Self {
            accounts: RwLock::default(),
            slots: tokio::sync::Semaphore::new(CONCURRENCY),
        }
    }
}

struct Observation {
    at: Instant,
    status: NodeStatus,
    health: NodeHealth,
    available: bool,
    lease_expires_at: Option<i64>,
    last_heartbeat_at: Option<i64>,
}

impl ComputerObservations {
    pub(crate) fn apply(&self, account: &str, info: &mut NodeInfo) {
        info.health = NodeHealth::Unknown;
        info.available = false;
        info.lease_expires_at = None;
        info.last_heartbeat_at = None;
        let accounts = self.accounts.read().expect("Computer observations lock");
        if let Some(observation) = accounts
            .get(account)
            .and_then(|nodes| nodes.get(&info.node_id))
            .filter(|observation| observation.at.elapsed() <= MAX_AGE)
        {
            info.status = observation.status;
            info.health = observation.health;
            info.available = observation.available;
            info.lease_expires_at = observation.lease_expires_at;
            info.last_heartbeat_at = observation.last_heartbeat_at;
        }
    }

    fn record(&self, account: &str, info: NodeInfo) {
        self.accounts
            .write()
            .expect("Computer observations lock")
            .entry(account.into())
            .or_default()
            .insert(
                info.node_id,
                Observation {
                    at: Instant::now(),
                    status: info.status,
                    health: info.health,
                    available: info.available,
                    lease_expires_at: info.lease_expires_at,
                    last_heartbeat_at: info.last_heartbeat_at,
                },
            );
    }
}

struct AccountLifetime {
    host: Weak<ApplicationSyncHost>,
    account: String,
}
impl Drop for AccountLifetime {
    fn drop(&mut self) {
        if let Some(host) = self.host.upgrade() {
            host.computer_observations
                .accounts
                .write()
                .expect("Computer observations lock")
                .remove(&self.account);
        }
    }
}

pub(crate) async fn maintain(
    host: Weak<ApplicationSyncHost>,
    mut actors: tokio::sync::watch::Receiver<Actor>,
) {
    let _lifetime = AccountLifetime {
        host: host.clone(),
        account: actors.borrow().user_id.clone(),
    };
    let mut interval = tokio::time::interval_at(
        tokio::time::Instant::now() + REFRESH_INTERVAL,
        REFRESH_INTERVAL,
    );
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let actor = actors.borrow_and_update().clone();
        let refresh = async {
            let Some(host) = host.upgrade() else {
                return Ok(());
            };
            if host.account_connections_allowed(&actor.user_id)
                && host.config.agent_node.control_plane.is_none()
            {
                host.refresh_computer_observations(&actor).await?;
            }
            Ok::<_, talos_error::TalosError>(())
        };
        tokio::select! {
            result = refresh => {
                if let Err(error) = result { talos_logging::report_error(&error); }
            },
            changed = actors.changed() => { if changed.is_err() { break } else { continue } },
        }
        tokio::select! {
            _ = interval.tick() => {},
            changed = actors.changed() => { if changed.is_err() { break } },
        }
    }
}

impl ApplicationSyncHost {
    pub(crate) async fn refresh_computer_observations(&self, actor: &Actor) -> TalosResult<()> {
        let verified = self.verified_actor(actor).await?;
        let client = self.gateway_client().await?;
        let local = self.local_computer_id().await?;
        let started = Instant::now();
        let mut cursor = None;
        loop {
            let page = self
                .service
                .nodes
                .list_computers(
                    actor,
                    ComputerListRequest {
                        cursor,
                        limit: Some(50),
                        query: None,
                    },
                )
                .await?;
            let mut results = stream::iter(page.data.into_iter().filter(|info|
                info.node_id != "embedded" && info.node_id != local && info.managed_worker_phase.is_none()
            ).map(|mut info| {
                let client = &client;
                let verified = &verified;
                async move {
                    let _slot = self.computer_observations.slots.acquire().await.expect("observation semaphore");
                    let result = tokio::time::timeout(RPC_TIMEOUT,
                        client.get_node(verified.access_token.as_deref().expect("verified token"), &info.node_id)
                    ).await;
                    match result {
                        Ok(Ok(live)) if live.node_id == info.node_id && live.owner_user_id.as_deref() == Some(&verified.user_id) => {
                            self.computer_observations.record(&verified.user_id, live);
                        },
                        Ok(Err(error)) if error.is_code(ErrorCode::ResourceExpired) => {
                            self.sync_stores.nodes.reconcile_deleted_registration(&info.node_id, &verified.user_id).await?;
                        },
                        Ok(Ok(_)) => return Err(talos_error::TalosError::new(
                            ErrorCode::PermissionDenied, "The control-plane Computer observation has a different owner or identity.")),
                        Ok(Err(error)) if matches!(error.error_code(), ErrorCode::Unauthenticated | ErrorCode::PermissionDenied) => return Err(error),
                        failed => {
                            match failed {
                                Ok(Err(error)) => tracing::debug!(computer_id = %info.node_id, code = ?error.error_code(), "Control-plane Computer observation unavailable"),
                                Err(_) => tracing::debug!(computer_id = %info.node_id, "Control-plane Computer observation timed out"),
                                Ok(Ok(_)) => unreachable!("successful observations handled above"),
                            }
                            info.available = false;
                            info.health = NodeHealth::Unknown;
                            info.lease_expires_at = None;
                            info.last_heartbeat_at = None;
                            self.computer_observations.record(&verified.user_id, info);
                        },
                    }
                    Ok::<_, talos_error::TalosError>(())
                }
            })).buffer_unordered(CONCURRENCY);
            while let Some(result) = results.next().await {
                result?;
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        if let Some(nodes) = self
            .computer_observations
            .accounts
            .write()
            .expect("Computer observations lock")
            .get_mut(&verified.user_id)
        {
            nodes.retain(|_, observation| observation.at >= started);
        }
        Ok(())
    }
}
