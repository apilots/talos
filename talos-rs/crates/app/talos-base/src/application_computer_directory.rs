//! Account-owned Server directory cache, independent of document synchronization.
use super::application_sync::ApplicationSyncHost;
use std::{
    collections::{HashMap, HashSet},
    sync::{RwLock, Weak},
    time::{Duration, Instant},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_service::Actor;
use talos_types::{NodeHealth, NodeInfo};
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const MAX_AGE: Duration = Duration::from_secs(15);
const RPC_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct ComputerDirectory {
    accounts: RwLock<HashMap<String, AccountDirectory>>,
    slots: tokio::sync::Semaphore,
    publish: tokio::sync::Mutex<()>,
}
#[derive(Default)]
struct AccountDirectory {
    complete: bool,
    started_at: Option<Instant>,
    published_at: Option<Instant>,
    nodes: HashMap<String, Observation>,
}
struct Observation {
    at: Instant,
    info: Option<NodeInfo>,
}
impl Default for ComputerDirectory {
    fn default() -> Self {
        Self {
            accounts: RwLock::default(),
            slots: tokio::sync::Semaphore::new(4),
            publish: tokio::sync::Mutex::new(()),
        }
    }
}
impl ComputerDirectory {
    pub(crate) async fn clear_account(&self, account: &str) {
        let _publish = self.publish.lock().await;
        self.accounts
            .write()
            .expect("Computer directory cache lock")
            .remove(account);
    }

    pub(crate) fn apply(&self, account: &str, info: &mut NodeInfo) -> TalosResult<()> {
        let accounts = self.accounts.read().expect("Computer directory cache lock");
        let directory = accounts.get(account);
        let observed = directory.and_then(|value| value.nodes.get(&info.node_id));
        if directory.is_some_and(|value| value.complete) && observed.is_none() {
            return Err(TalosError::new(
                ErrorCode::ResourceExpired,
                "Computer is absent from the account's control-plane directory.",
            ));
        }
        if let Some(observed) = observed {
            let Some(live) = &observed.info else {
                return Err(TalosError::new(
                    ErrorCode::ResourceExpired,
                    "Computer was deleted at the control plane.",
                ));
            };
            *info = live.clone();
        }
        if observed.is_none_or(|value| value.at.elapsed() > MAX_AGE) {
            info.health = NodeHealth::Unknown;
            info.available = false;
            info.lease_expires_at = None;
            info.last_heartbeat_at = None;
        }
        Ok(())
    }
    fn observation_after(
        &self,
        account: &str,
        id: &str,
        started: Instant,
    ) -> TalosResult<Option<NodeInfo>> {
        let accounts = self.accounts.read().expect("Computer directory cache lock");
        if let Some(directory) = accounts.get(account) {
            if let Some(observed) = directory.nodes.get(id) {
                if observed.at > started {
                    return observed.info.clone().map(Some).ok_or_else(|| {
                        TalosError::new(
                            ErrorCode::ResourceExpired,
                            "Computer was deleted at the control plane.",
                        )
                    });
                }
            } else if directory.complete && directory.published_at.is_some_and(|at| at > started) {
                return Err(TalosError::new(
                    ErrorCode::ResourceExpired,
                    "Computer is absent from the control-plane directory.",
                ));
            }
        }
        Ok(None)
    }
    fn record(&self, account: &str, info: NodeInfo) {
        self.accounts
            .write()
            .expect("Computer directory cache lock")
            .entry(account.into())
            .or_default()
            .nodes
            .insert(
                info.node_id.clone(),
                Observation {
                    at: Instant::now(),
                    info: Some(info),
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
            host.computer_directory
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
                host.refresh_computer_directory(&actor).await?;
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
    /// Publish an authenticated Server result without replacing newer directory facts.
    pub(crate) async fn cache_server_computer(
        &self,
        account: &str,
        info: &NodeInfo,
        started: Instant,
    ) -> TalosResult<NodeInfo> {
        let _publish = self.computer_directory.publish.lock().await;
        if !self.account_connections_allowed(account) {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "The Computer account is no longer active.",
            ));
        }
        if let Some(current) =
            self.computer_directory
                .observation_after(account, &info.node_id, started)?
        {
            return Ok(current);
        }
        self.publish_computer(account, info).await
    }

    /// Caller holds the directory publication lock through persistence and observation.
    async fn publish_computer(&self, account: &str, info: &NodeInfo) -> TalosResult<NodeInfo> {
        self.sync_stores.nodes.confirm_remote_computer(info).await?;
        self.computer_directory.record(account, info.clone());
        Ok(info.clone())
    }
    /// Write confirmed management results through the same cache publication owner.
    pub(crate) async fn cache_confirmed_computer(
        &self,
        actor: &Actor,
        id: &str,
        info: &NodeInfo,
        started: Instant,
    ) -> TalosResult<NodeInfo> {
        if info.node_id != id || info.owner_user_id.as_deref() != Some(&actor.user_id) {
            return Err(TalosError::authorization_denied());
        }
        let _publish = self.computer_directory.publish.lock().await;
        if !self.account_connections_allowed(&actor.user_id) {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "The account Computer directory is no longer active.",
            ));
        }
        let mut confirmed = info.clone();
        if !matches!(
            self.computer_directory
                .observation_after(&actor.user_id, id, started),
            Ok(None)
        ) {
            // Request start cannot order remote commits. Resolve overlap against
            // Server while publication is serialized, including connection setup.
            let current = tokio::time::timeout(RPC_TIMEOUT, async {
                let token = actor.access_token.as_deref().ok_or_else(|| {
                    TalosError::new(
                        ErrorCode::Unauthenticated,
                        "Computer management requires a signed-in account.",
                    )
                })?;
                self.gateway_client().await?.get_node(token, id).await
            })
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Confirm the current Computer after overlapping directory updates.",
                    error,
                )
            })?;
            if !self.account_connections_allowed(&actor.user_id) {
                return Err(TalosError::new(
                    ErrorCode::Unauthenticated,
                    "The account Computer directory is no longer active.",
                ));
            }
            confirmed = match current {
                Ok(current) => current,
                Err(error)
                    if matches!(
                        error.error_code(),
                        ErrorCode::ResourceNotFound | ErrorCode::ResourceExpired
                    ) =>
                {
                    self.publish_computer_deletion(&actor.user_id, id).await?;
                    return Err(if error.is_code(ErrorCode::ResourceExpired) {
                        error
                    } else {
                        TalosError::localized(
                            talos_error::ErrorLocalization::ComputerRegistrationRemoved,
                        )
                    });
                }
                Err(error) => return Err(error),
            };
            if confirmed.node_id != id || confirmed.owner_user_id.as_deref() != Some(&actor.user_id)
            {
                return Err(TalosError::authorization_denied());
            }
        }
        self.publish_computer(&actor.user_id, &confirmed).await
    }
    pub(crate) async fn cache_deleted_computer(&self, actor: &Actor, id: &str) -> TalosResult<()> {
        let _publish = self.computer_directory.publish.lock().await;
        if !self.account_connections_allowed(&actor.user_id) {
            return Err(TalosError::new(
                ErrorCode::Unauthenticated,
                "The account Computer directory is no longer active.",
            ));
        }
        self.publish_computer_deletion(&actor.user_id, id).await
    }

    /// Caller holds the directory publication lock.
    async fn publish_computer_deletion(&self, account: &str, id: &str) -> TalosResult<()> {
        self.sync_stores
            .nodes
            .remove_cached_computer(account, id)
            .await?;
        self.computer_directory
            .accounts
            .write()
            .expect("Computer directory cache lock")
            .entry(account.into())
            .or_default()
            .nodes
            .insert(
                id.into(),
                Observation {
                    at: Instant::now(),
                    info: None,
                },
            );
        Ok(())
    }
    pub(crate) async fn refresh_computer_directory(&self, actor: &Actor) -> TalosResult<()> {
        let verified = self.verified_actor(actor).await?;
        let client = self.gateway_client().await?;
        let started = Instant::now();
        let mut cursor = None;
        let mut cursors = HashSet::new();
        let mut incoming = HashMap::new();
        loop {
            let _slot = self
                .computer_directory
                .slots
                .acquire()
                .await
                .expect("directory semaphore");
            let page = tokio::time::timeout(
                RPC_TIMEOUT,
                client.list_computers(
                    verified.access_token.as_deref().expect("verified token"),
                    cursor,
                ),
            )
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Refresh the account Computer directory.",
                    error,
                )
            })??;
            for info in page.data {
                if info.node_id == "embedded"
                    || info.owner_user_id.as_deref() != Some(&verified.user_id)
                {
                    return Err(TalosError::authorization_denied());
                }
                if incoming.insert(info.node_id.clone(), info).is_some() {
                    return Err(TalosError::new(
                        ErrorCode::Conflict,
                        "The control-plane directory contains duplicate Computer identities.",
                    ));
                }
            }
            cursor = page.next_cursor;
            let Some(next) = &cursor else { break };
            if !cursors.insert(next.clone()) {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "The control-plane directory repeated a page cursor.",
                ));
            }
        }
        let _publish = self.computer_directory.publish.lock().await;
        if self
            .computer_directory
            .accounts
            .read()
            .expect("Computer directory cache lock")
            .get(&actor.user_id)
            .and_then(|value| value.started_at)
            .is_some_and(|previous| previous > started)
        {
            return Ok(());
        }
        // Keep confirmations newer than this pull, including deletions.
        let mut merged: HashMap<String, Observation> = incoming
            .into_iter()
            .map(|(id, info)| {
                (
                    id,
                    Observation {
                        at: Instant::now(),
                        info: Some(info),
                    },
                )
            })
            .collect();
        if let Some(previous) = self
            .computer_directory
            .accounts
            .read()
            .expect("Computer directory cache lock")
            .get(&actor.user_id)
        {
            for (id, value) in &previous.nodes {
                if value.at > started {
                    merged.insert(
                        id.clone(),
                        Observation {
                            at: value.at,
                            info: value.info.clone(),
                        },
                    );
                }
            }
        }
        let visible: Vec<_> = merged
            .values()
            .filter_map(|value| value.info.clone())
            .collect();
        self.sync_stores
            .nodes
            .replace_remote_computers(&actor.user_id, &visible)
            .await?;
        self.computer_directory
            .accounts
            .write()
            .expect("Computer directory cache lock")
            .insert(
                actor.user_id.clone(),
                AccountDirectory {
                    complete: true,
                    started_at: Some(started),
                    published_at: Some(Instant::now()),
                    nodes: merged,
                },
            );

        Ok(())
    }
}
