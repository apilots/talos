use crate::BrowserAction;
use crate::BrowserActionExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
#[path = "desktop.rs"]
pub mod desktop;
#[path = "environment.rs"]
mod environment;
#[path = "invocation.rs"]
mod invocation;
pub use environment::PreparedBrowserEnvironment;
#[path = "control.rs"]
mod operations;
pub use invocation::{BrowserDispatch, BrowserInvocationLease};

pub type BrowserFuture<'a, T> = Pin<Box<dyn Future<Output = TalosResult<T>> + Send + 'a>>;
pub type PluginEnabledReader = Arc<dyn Fn() -> TalosResult<bool> + Send + Sync>;

/// Concrete browser backend only; no product transport or node routing.
pub trait BrowserBackend: Send + Sync {
    fn refresh(&self, _user: String) -> BrowserFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn readiness(&self, target: BrowserTargetKind) -> BrowserReadiness;
    fn desktop_available(&self) -> bool {
        false
    }
    fn execute(
        &self,
        action: BrowserAction,
        dispatch: BrowserDispatch,
    ) -> BrowserFuture<'_, McpCallToolResponse>;
    fn connect(&self, _user: String) -> BrowserFuture<'_, ()>;
    fn disconnect(&self, _user: String) -> BrowserFuture<'_, ()>;
    fn open_local(&self, _user: String) -> BrowserFuture<'_, ()>;
    fn downloads_saved(
        &self,
        _user: String,
        _target: BrowserTargetKind,
        _files: Vec<talos_types::ToolOutputFile>,
    ) -> BrowserFuture<'_, ()> {
        Box::pin(async {
            Err(unavailable(
                "Browser download acknowledgement is unavailable.",
            ))
        })
    }
    fn release_inputs(&self) -> BrowserFuture<'_, ()>;
    fn shutdown(&self) -> BrowserFuture<'_, ()>;
    fn desktop(&self) -> BrowserFuture<'_, tokio::net::TcpStream> {
        Box::pin(async {
            Err(unavailable(
                "This Computer has no remote desktop transport.",
            ))
        })
    }
}

/// Supplied only by the real ToolContext owner, never by model arguments.
#[derive(Clone, Debug)]
pub struct BrowserInvocationIdentity {
    pub user_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub call_id: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Preferences {
    selected: BrowserTargetKind,
    manual_hold: bool,
}

struct Invocation {
    identity: BrowserInvocationIdentity,
    digest: [u8; 32],
    epoch: u64,
    target: BrowserTargetKind,
    deadline: Instant,
    upload: Option<talos_types::ToolInputFile>,
    downloads: Vec<talos_types::ToolOutputFile>,
    executing: bool,
    cancellation: CancellationToken,
}
struct State {
    host: BrowserHostKind,
    preferences: Preferences,
    generation: String,
    revision: u64,
    epoch: u64,
    owner: BrowserControlOwner,
    invocations: HashMap<String, Invocation>,
    agent_user: Option<String>,
    revoked_users: HashSet<String>,
    views: HashMap<String, desktop::View>,
    controller_stream: Option<String>,
    desktop_cleanup_error: Option<talos_error::ApiError>,
}

pub struct BrowserRuntime {
    state: Mutex<State>,
    backend: RwLock<Option<Arc<dyn BrowserBackend>>>,
    input: Arc<AsyncMutex<()>>,
    preferences_path: PathBuf,
    mutations: tokio_util::task::TaskTracker,
    enabled: PluginEnabledReader,
    shutdown: CancellationToken,
    desktop_connector: RwLock<Option<desktop::DesktopConnector>>,
    pub(crate) ipc_tasks: tokio_util::task::TaskTracker,
    managed_environment: Mutex<environment::ManagedEnvironmentBinding>,
    restart_required: std::sync::atomic::AtomicBool,
}

impl BrowserRuntime {
    pub fn new(
        home: PathBuf,
        host: BrowserHostKind,
        enabled: PluginEnabledReader,
    ) -> TalosResult<Arc<Self>> {
        let path = home.join("browser/state.json");
        let preferences = if path.exists() {
            talos_fs::read_json(&path)?
        } else {
            Preferences {
                selected: if matches!(host, BrowserHostKind::Docker | BrowserHostKind::Desktop) {
                    BrowserTargetKind::Managed
                } else {
                    BrowserTargetKind::Connected
                },
                manual_hold: false,
            }
        };
        let owner = if preferences.manual_hold {
            BrowserControlOwner::Suspended {
                reason: BrowserSuspensionReason::WorkerRestarted,
            }
        } else {
            BrowserControlOwner::Idle
        };
        Ok(Arc::new(Self {
            state: Mutex::new(State {
                host,
                preferences,
                generation: Uuid::new_v4().to_string(),
                revision: 0,
                epoch: 0,
                owner,
                invocations: HashMap::new(),
                agent_user: None,
                revoked_users: HashSet::new(),
                views: HashMap::new(),
                controller_stream: None,
                desktop_cleanup_error: None,
            }),
            backend: RwLock::new(None),
            input: Arc::new(AsyncMutex::new(())),
            preferences_path: path,
            mutations: tokio_util::task::TaskTracker::new(),
            enabled,
            shutdown: CancellationToken::new(),
            desktop_connector: RwLock::new(None),
            ipc_tasks: tokio_util::task::TaskTracker::new(),
            managed_environment: Mutex::new(Default::default()),
            restart_required: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    pub fn set_backend(&self, backend: Arc<dyn BrowserBackend>) -> TalosResult<()> {
        let mut slot = self.backend.write().map_err(|_| poisoned())?;
        if slot.is_some() {
            return Err(conflict("The browser backend is already bound."));
        }
        *slot = Some(backend);
        Ok(())
    }
    fn backend(&self) -> TalosResult<Arc<dyn BrowserBackend>> {
        if self
            .restart_required
            .load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(unavailable(
                "Browser dependencies changed. Restart this Worker before continuing.",
            ));
        }
        self.backend
            .read()
            .map_err(|_| poisoned())?
            .clone()
            .ok_or_else(|| {
                unavailable(
                    "This Computer's browser backend is not available. Check Browser settings.",
                )
            })
    }
    pub fn snapshot(&self) -> TalosResult<BrowserSnapshot> {
        let enabled = (self.enabled)()?;
        let state = self.state.lock().map_err(|_| poisoned())?;
        let kinds: &[BrowserTargetKind] = match state.host {
            BrowserHostKind::Docker => &[BrowserTargetKind::Managed],
            BrowserHostKind::Desktop => &[BrowserTargetKind::Managed, BrowserTargetKind::Connected],
            BrowserHostKind::Native => &[BrowserTargetKind::Connected],
            BrowserHostKind::Server => &[],
        };
        let backend = if self
            .restart_required
            .load(std::sync::atomic::Ordering::Acquire)
        {
            None
        } else {
            self.backend.read().map_err(|_| poisoned())?.clone()
        };
        let targets = kinds
            .iter()
            .map(|kind| BrowserTarget {
                target_id: target_id(*kind).into(),
                kind: *kind,
                display_name: match kind {
                    BrowserTargetKind::Managed => "Talos browser",
                    BrowserTargetKind::Connected => "My Chrome",
                }
                .into(),
                local_open: match (state.host, kind) {
                    (BrowserHostKind::Desktop, BrowserTargetKind::Managed) => {
                        BrowserLocalOpenCapability::DesktopOwned
                    }
                    (_, BrowserTargetKind::Connected) => {
                        BrowserLocalOpenCapability::ExternalUserWindow
                    }
                    _ => BrowserLocalOpenCapability::Unavailable,
                },
                readiness: match require_private_ipc_support() {
                    Err(error) => BrowserReadiness::Unavailable {
                        error: error.to_api_error(),
                    },
                    Ok(()) => backend.as_ref().map_or_else(
                        || BrowserReadiness::Unavailable {
                            error: unavailable(if self.restart_required.load(std::sync::atomic::Ordering::Acquire) {
                                "Browser dependencies changed. Restart this Worker before continuing."
                            } else { "This Computer's browser backend is unavailable." }).to_api_error(),
                        },
                        |backend| backend.readiness(*kind),
                    ),
                },
            })
            .collect::<Vec<_>>();
        let selected_target_id = kinds
            .contains(&state.preferences.selected)
            .then(|| target_id(state.preferences.selected).to_string());
        Ok(BrowserSnapshot {
            enabled,
            host: state.host,
            selected_target_id,
            targets,
            display: if state.host == BrowserHostKind::Docker
                && backend
                    .as_ref()
                    .is_some_and(|backend| backend.desktop_available())
            {
                if self
                    .desktop_connector
                    .read()
                    .map_err(|_| poisoned())?
                    .is_some()
                {
                    BrowserDisplayCapability::DockerDesktop {
                        target_id: "managed".into(),
                    }
                } else {
                    BrowserDisplayCapability::Unavailable {
                        reason: BrowserDisplayUnavailableReason::HostPolicy,
                    }
                }
            } else {
                BrowserDisplayCapability::Unavailable {
                    reason: if matches!(
                        state.host,
                        BrowserHostKind::Server | BrowserHostKind::Docker
                    ) {
                        BrowserDisplayUnavailableReason::NotBundled
                    } else {
                        BrowserDisplayUnavailableReason::NativeHost
                    },
                }
            },
            control: control(&state),
        })
    }

    /// A trusted local account logout revokes late tool calls before closing owned websites.
    pub async fn reset_account(&self, user_id: &str) -> TalosResult<()> {
        self.state
            .lock()
            .map_err(|_| poisoned())?
            .revoked_users
            .insert(user_id.into());
        let paused = self.pause(None, BrowserSuspensionReason::UserPaused).await;
        let closed = self.backend.read().map_err(|_| poisoned())?.clone();
        let closed = match closed {
            Some(backend) => backend.shutdown().await,
            None => Ok(()),
        };
        paused.and(closed)
    }
    fn persist(&self, preferences: &Preferences) -> TalosResult<()> {
        if let Some(parent) = self.preferences_path.parent() {
            talos_fs::create_directory(parent, true)?;
        }
        talos_fs::write_json_pretty(&self.preferences_path, preferences)
    }
    pub fn finish_turn(&self, session_id: &str, turn_id: &str) {
        if let Ok(mut s) = self.state.lock() {
            let tokens = s
                .invocations
                .iter()
                .filter(|(_, r)| {
                    r.identity.session_id == session_id && r.identity.turn_id == turn_id
                })
                .map(|(key, _)| key.clone())
                .collect::<Vec<_>>();
            for token in tokens {
                if let Some(r) = s.invocations.remove(&token) {
                    r.cancellation.cancel();
                }
            }
            if matches!(&s.owner,BrowserControlOwner::Agent{session_id:sid,turn_id:tid} if sid==session_id&&tid==turn_id)
            {
                s.owner = BrowserControlOwner::Idle;
                s.agent_user = None;
                s.revision += 1;
            }
        }
    }
    /// Called only after the original plugin commit guard and durable disable succeed.
    pub async fn plugin_disabled(&self) -> TalosResult<()> {
        self.stop_desktop_views(false)?;
        {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            revoke(&mut s);
            s.owner = BrowserControlOwner::Suspended {
                reason: BrowserSuspensionReason::PluginDisabled,
            };
            s.agent_user = None;
            self.mutations.close();
        }
        tokio::time::timeout(Duration::from_secs(5), self.mutations.wait())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Browser calls did not settle after disable.",
                    error,
                )
            })?;
        let _input = tokio::time::timeout(Duration::from_secs(5), self.input.clone().lock_owned())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::Timeout,
                    "Browser disable fencing timed out.",
                    error,
                )
            })?;
        let backend = self.backend.read().map_err(|_| poisoned())?.clone();
        if let Some(backend) = backend {
            backend.release_inputs().await?;
            backend.shutdown().await?;
            self.state
                .lock()
                .map_err(|_| poisoned())?
                .desktop_cleanup_error = None;
        }
        Ok(())
    }
    pub async fn plugin_enabled(&self) -> TalosResult<()> {
        self.check_desktop_cleanup()?;
        let _input = self.input.clone().lock_owned().await;
        if let Ok(backend) = self.backend() {
            backend.release_inputs().await?;
        }
        let mut s = self.state.lock().map_err(|_| poisoned())?;
        revoke(&mut s);
        s.agent_user = None;
        s.owner = if s.preferences.manual_hold {
            BrowserControlOwner::Suspended {
                reason: BrowserSuspensionReason::UserPaused,
            }
        } else {
            self.mutations.reopen();
            BrowserControlOwner::Idle
        };
        Ok(())
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        self.shutdown.cancel();
        self.ipc_tasks.close();
        self.ipc_tasks.wait().await;
        self.stop_desktop_views(false)?;
        self.mutations.close();
        {
            let mut s = self.state.lock().map_err(|_| poisoned())?;
            revoke(&mut s);
        }
        let settled = tokio::time::timeout(Duration::from_secs(5), self.mutations.wait())
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::RunCleanupUnconfirmed,
                    "Browser invocations did not settle before shutdown.",
                    error,
                )
            });
        let stopped = if let Ok(backend) = self.backend() {
            backend.shutdown().await
        } else {
            Ok(())
        };
        settled.and(stopped)
    }
    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }
}

fn revoke(s: &mut State) {
    for view in s.views.values() {
        if view.binding.input_mode == BrowserInputMode::Control {
            view.stop.cancel();
        }
    }
    s.controller_stream = None;
    s.epoch += 1;
    s.revision += 1;
    for r in s.invocations.values() {
        r.cancellation.cancel();
    }
}
fn control(s: &State) -> BrowserControlState {
    BrowserControlState {
        generation: s.generation.clone(),
        revision: s.revision,
        epoch: s.epoch,
        owner: s.owner.clone(),
    }
}
pub(crate) fn target_id(kind: BrowserTargetKind) -> &'static str {
    match kind {
        BrowserTargetKind::Managed => "managed",
        BrowserTargetKind::Connected => "connected",
    }
}
fn revision(s: &State, expected: u64) -> TalosResult<()> {
    if s.revision != expected {
        Err(conflict("Browser state changed. Refresh before retrying."))
    } else {
        Ok(())
    }
}
fn poisoned() -> TalosError {
    TalosError::internal("Browser state lock is poisoned.")
}
fn denied() -> TalosError {
    TalosError::new(
        ErrorCode::PermissionDenied,
        "Browser invocation authority is missing, expired or revoked.",
    )
}
fn conflict(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::Conflict, message)
}
fn unavailable(message: &'static str) -> TalosError {
    TalosError::new(ErrorCode::ServiceUnavailable, message)
}

// Every advertised backend is reached by the bundled MCP through this transport.
// A connected Desktop host cannot make an unsupported MCP transport usable.
fn require_private_ipc_support() -> TalosResult<()> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(unavailable(
            "Browser IPC is not supported on this platform.",
        ))
    }
}
