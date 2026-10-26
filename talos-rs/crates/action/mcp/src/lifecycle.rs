mod connection;
mod worker;

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpCallToolRequest, McpCallToolResponse, McpReadResourceRequest, McpReadResourceResponse,
    McpServerInfo, McpServerRuntimeDescriptor, McpServerStatus, McpToolDescriptor,
};
use tokio::sync::Semaphore;
use tokio_util::{
    sync::CancellationToken,
    task::{task_tracker::TaskTrackerToken, TaskTracker},
};

use crate::{
    protocol::{self, McpClient},
    CLEANUP_TIMEOUT, DEFAULT_TOOL_TIMEOUT,
};

/// Owns the current MCP connections. Reconciliation publishes definitions without
/// waiting for external servers; callers must explicitly shut down this owner.
#[derive(Clone)]
pub struct TalosMcpRuntime(Arc<Connections>);

/// Connection-only inputs. Secret values are never folded into the serializable definition.
#[derive(Clone)]
pub struct McpConnectionInput {
    pub descriptor: McpServerRuntimeDescriptor,
    pub credentials: Option<crate::McpConnectionCredentials>,
    pub startup_error: Option<TalosError>,
}
impl From<McpServerRuntimeDescriptor> for McpConnectionInput {
    fn from(descriptor: McpServerRuntimeDescriptor) -> Self {
        Self {
            descriptor,
            credentials: None,
            startup_error: None,
        }
    }
}

struct Connections {
    servers: RwLock<BTreeMap<String, Arc<Server>>>,
    cleanup: Arc<crate::cleanup::Cleanup>,
    startup_slots: Arc<Semaphore>,
    browser_socket: Option<std::path::PathBuf>,
}

struct Server {
    descriptor: McpServerRuntimeDescriptor,
    credentials: Option<crate::McpConnectionCredentials>,
    preflight_failed: bool,
    abort_calls: CancellationToken,
    state: RwLock<State>,
    retire: CancellationToken,
    calls: TaskTracker,
}

enum State {
    NotStarted,
    Starting,
    Ready {
        client: McpClient,
        tools: Arc<Vec<McpToolDescriptor>>,
    },
    Failed(TalosError),
    Exited(TalosError),
}

/// An immutable binding used for both tool authorization and execution.
/// Replacement or retirement rejects new admission through an old binding.
#[derive(Clone)]
pub struct McpConnection {
    server: Arc<Server>,
    client: McpClient,
    tools: Arc<Vec<McpToolDescriptor>>,
}

impl Default for TalosMcpRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl TalosMcpRuntime {
    pub fn new() -> Self {
        Self::with_builtin_browser_socket(None)
    }

    /// Per-process private bridge, never serialized in an MCP definition.
    pub fn with_builtin_browser_socket(socket: Option<std::path::PathBuf>) -> Self {
        Self(Arc::new(Connections {
            servers: RwLock::new(BTreeMap::new()),
            cleanup: Arc::default(),
            startup_slots: Arc::new(Semaphore::new(4)),
            browser_socket: socket,
        }))
    }

    /// Validate before persistence. The application serializes preparation,
    /// durable commit and publication with configuration changes and shutdown.
    pub fn prepare(&self, descriptors: Vec<McpConnectionInput>) -> TalosResult<PreparedMcpUpdate> {
        let mut desired = BTreeMap::new();
        for mut descriptor in descriptors {
            let builtin = matches!(&descriptor.descriptor.source,
                talos_types::McpServerSource::Plugin { plugin_id, .. }
                if plugin_id.0 == talos_types::OFFICIAL_BROWSER_PLUGIN_ID);
            if builtin {
                let valid = descriptor.descriptor.server.name
                    == talos_types::BROWSER_MCP_SERVER_NAME
                    && descriptor.descriptor.server.credentials.is_none()
                    && matches!(&descriptor.descriptor.server.transport,
                        talos_types::McpTransportDefinition::Stdio { command, args, env, .. }
                        if std::env::current_exe().is_ok_and(|path| path == std::path::Path::new(command))
                            && args == &["--browser-mcp"] && env.is_empty());
                if !valid || descriptor.credentials.is_some() {
                    return Err(TalosError::new(
                        ErrorCode::PermissionDenied,
                        "The bundled Browser definition does not match this Talos executable.",
                    ));
                }
                if let Some(socket) = &self.0.browser_socket {
                    descriptor.credentials = Some(crate::McpConnectionCredentials::Token(
                        talos_types::McpCredentialValues {
                            authentication: talos_types::McpAuthentication::None,
                            token: None,
                            headers: None,
                            env: Some(std::collections::BTreeMap::from([(
                                "TALOS_BROWSER_SOCKET".into(),
                                socket.to_string_lossy().into_owned(),
                            )])),
                        },
                    ));
                }
            } else if descriptor.descriptor.server.name == talos_types::BROWSER_MCP_SERVER_NAME {
                return Err(TalosError::new(
                    ErrorCode::PermissionDenied,
                    "The Browser MCP name is reserved for the bundled plugin.",
                ));
            }
            talos_types::validate_mcp_server_name(&descriptor.descriptor.server.name)?;
            if desired
                .insert(descriptor.descriptor.server.name.clone(), descriptor)
                .is_some()
            {
                return Err(TalosError::new(
                    ErrorCode::Conflict,
                    "MCP service names must be unique.",
                ));
            }
        }
        if self.0.cleanup.tasks.is_closed() {
            return Err(TalosError::new(
                ErrorCode::Cancelled,
                "MCP connections are shutting down.",
            ));
        }
        Ok(PreparedMcpUpdate {
            runtime: self.clone(),
            desired,
        })
    }

    /// Lists current connections with bounded, secret-free summaries. A cursor is
    /// the last returned name; removals between pages continue in lexical order.
    pub fn list_servers(
        &self,
        request: talos_types::McpServerListRequest,
    ) -> TalosResult<talos_types::McpServerListResponse> {
        let limit = request.limit.unwrap_or(50);
        if !(1..=200).contains(&limit) {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                "MCP list limit must be between 1 and 200.",
            ));
        }
        if request
            .cursor
            .as_ref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > 256)
        {
            return Err(TalosError::new(
                ErrorCode::InvalidCursor,
                "The MCP server cursor is invalid.",
            ));
        }
        let servers = self.0.servers.read().expect("MCP connection lock poisoned");
        let mut data: Vec<_> = servers
            .iter()
            .filter(|(name, _)| request.cursor.as_ref().is_none_or(|cursor| *name > cursor))
            .take(limit + 1)
            .map(|(_, server)| server.summary())
            .collect();
        let next_cursor = if data.len() > limit {
            data.truncate(limit);
            data.last().map(|server| server.name.clone())
        } else {
            None
        };
        Ok(talos_types::McpServerListResponse { data, next_cursor })
    }

    /// Capture complete ready bindings under short synchronous locks. No external
    /// I/O or caller callback runs while these locks are held.
    pub fn connections(&self) -> Vec<McpConnection> {
        self.0
            .servers
            .read()
            .expect("MCP connection lock poisoned")
            .values()
            .filter_map(|server| server.connection())
            .collect()
    }

    pub fn connection(&self, name: &str) -> TalosResult<McpConnection> {
        self.0
            .servers
            .read()
            .expect("MCP connection lock poisoned")
            .get(name)
            .and_then(|server| server.connection())
            .ok_or_else(not_ready)
    }

    /// Invalidates a credential generation immediately, before private persistence is removed.
    pub fn revoke_credentials(&self, definition: &talos_types::McpDefinitionRef) {
        let servers = self.0.servers.read().expect("MCP connection lock poisoned");
        for server in
            servers
                .values()
                .filter(|server| match (definition, &server.descriptor.source) {
                    (
                        talos_types::McpDefinitionRef::Plugin { install_id },
                        talos_types::McpServerSource::Plugin {
                            install_id: current,
                            ..
                        },
                    ) => install_id == current,
                })
        {
            server.abort_calls.cancel();
            server.retire();
        }
    }

    pub async fn shutdown(&self) -> TalosResult<()> {
        {
            let mut servers = self
                .0
                .servers
                .write()
                .expect("MCP connection lock poisoned");
            for server in std::mem::take(&mut *servers).into_values() {
                server.retire();
            }
            self.0.cleanup.tasks.close();
        }
        tokio::time::timeout(
            DEFAULT_TOOL_TIMEOUT + CLEANUP_TIMEOUT * 2,
            self.0.cleanup.tasks.wait(),
        )
        .await
        .map_err(|_| {
            TalosError::new(
                ErrorCode::RunCleanupUnconfirmed,
                "MCP connections did not finish shutdown within the deadline.",
            )
        })?;
        self.0.cleanup.result()
    }
}

/// A validated connection update with no external I/O during publication.
/// Shutdown is terminal: publication cannot revive a closed owner. Persistent
/// callers must serialize shutdown with their commit to keep those outcomes aligned.
pub struct PreparedMcpUpdate {
    runtime: TalosMcpRuntime,
    desired: BTreeMap<String, McpConnectionInput>,
}

impl PreparedMcpUpdate {
    pub fn publish(self) {
        let mut servers = self
            .runtime
            .0
            .servers
            .write()
            .expect("MCP connection lock poisoned");
        if self.runtime.0.cleanup.tasks.is_closed() {
            return;
        }
        servers.retain(|name, current| {
            let keep = self.desired.get(name).is_some_and(|next| {
                !current.retire.is_cancelled()
                    && next.descriptor == current.descriptor
                    && next.credentials == current.credentials
                    && next.startup_error.is_none()
                    && !current.preflight_failed
            });
            if !keep {
                if (current.descriptor.server.credentials.is_some()
                    || current.credentials.is_some())
                    && self.desired.get(name).is_none_or(|next| {
                        next.descriptor.server.credentials != current.descriptor.server.credentials
                            || next.credentials != current.credentials
                    })
                {
                    current.abort_calls.cancel();
                }
                current.retire();
            }
            keep
        });
        for (name, descriptor) in self.desired {
            if servers.contains_key(&name) {
                continue;
            }
            let startup_error = descriptor.startup_error;
            let server = Arc::new(Server {
                descriptor: descriptor.descriptor,
                credentials: descriptor.credentials,
                preflight_failed: startup_error.is_some(),
                abort_calls: CancellationToken::new(),
                state: RwLock::new(State::NotStarted),
                retire: CancellationToken::new(),
                calls: TaskTracker::new(),
            });
            servers.insert(name, server.clone());
            if let Some(error) = startup_error {
                server.fail(error);
                continue;
            }
            self.runtime.0.cleanup.tasks.spawn(server.run(
                self.runtime.0.startup_slots.clone(),
                self.runtime.0.cleanup.clone(),
            ));
        }
    }
}

impl Drop for Connections {
    fn drop(&mut self) {
        for server in self
            .servers
            .get_mut()
            .expect("MCP connection lock poisoned")
            .values()
        {
            server.retire();
        }
        self.cleanup.tasks.close();
    }
}

impl Server {
    fn retire(&self) {
        let _state = self.state.write().expect("MCP server lock poisoned");
        self.retire.cancel();
        self.calls.close();
    }

    fn connection(self: &Arc<Self>) -> Option<McpConnection> {
        let state = self.state.read().expect("MCP server lock poisoned");
        if self.retire.is_cancelled() {
            return None;
        }
        match &*state {
            State::Ready { client, tools } => Some(McpConnection {
                server: self.clone(),
                client: client.clone(),
                tools: tools.clone(),
            }),
            _ => None,
        }
    }

    fn summary(&self) -> McpServerInfo {
        let state = self.state.read().expect("MCP server lock poisoned");
        let (status, tool_count) = if self.retire.is_cancelled() {
            (McpServerStatus::NotStarted, 0)
        } else {
            match &*state {
                State::NotStarted => (McpServerStatus::NotStarted, 0),
                State::Starting => (McpServerStatus::Starting, 0),
                State::Ready { tools, .. } => (McpServerStatus::Ready, tools.len()),
                State::Exited(error) => (
                    McpServerStatus::Exited {
                        error: error.to_api_error(),
                    },
                    0,
                ),
                State::Failed(error) => (
                    McpServerStatus::Failed {
                        error: error.to_api_error(),
                    },
                    0,
                ),
            }
        };
        McpServerInfo {
            name: self.descriptor.server.name.clone(),
            display_name: self.descriptor.server.display_name.clone(),
            transport: self.descriptor.server.transport.kind(),
            source: self.descriptor.source.clone(),
            status,
            tool_count,
        }
    }

    fn fail(&self, error: TalosError) {
        let error = error.context(format!("mcp_server={}", self.descriptor.server.name));
        let mut state = self.state.write().expect("MCP server lock poisoned");
        talos_logging::report_error(&error);
        if !self.retire.is_cancelled() {
            *state = State::Failed(error);
        }
    }

    fn exited(&self, status: std::process::ExitStatus) {
        let mut state = self.state.write().expect("MCP server lock poisoned");
        if !self.retire.is_cancelled() {
            let error = TalosError::new(
                ErrorCode::ToolExecutionFailed,
                "The MCP server process exited unexpectedly.",
            )
            .context(format!(
                "mcp_server={}, exit_status={status}",
                self.descriptor.server.name
            ));
            talos_logging::report_error(&error);
            *state = State::Exited(error);
        }
    }
}

fn not_ready() -> TalosError {
    TalosError::new(
        ErrorCode::ToolExecutionFailed,
        "The MCP tool connection is no longer ready; refresh the available tools.",
    )
}
