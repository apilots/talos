//! Typed outbound dispatch delegates to the same direct RPC handlers.
use super::*;
use pb::agent_harness_rpc_server::AgentHarnessRpc;
use pb::worker_environment_rpc_server::WorkerEnvironmentRpc;
use pb::worker_rpc_call::Request as Call;
use pb::worker_rpc_result::Response as Reply;

impl AgentHarnessRpcAdapter {
    /// Authorize connection control frames against the adapter's current node lease.
    pub fn validate_connection_auth(&self, auth: &NodeAuth) -> TalosResult<()> {
        self.authorize(auth).map(|_| ())
    }

    /// Project the existing Harness RPC through Connect without another business model.
    /// The carrier owns framing, cancellation and bounds; this adapter validates every request.
    pub async fn dispatch_connect(
        &self,
        mut requests: RpcStream<Call>,
    ) -> Result<RpcStream<Reply>, Status> {
        let first = requests
            .next()
            .await
            .transpose()?
            .ok_or_else(|| status_from_error(invalid_request()))?;
        macro_rules! stream {
            ($value:expr, $variant:ident, $method:ident, $reply:ident) => {{
                let first = tokio_stream::once(Ok(*$value));
                let rest = requests.map(|frame| match frame? {
                    Call::$variant(value) => Ok(*value),
                    _ => Err(status_from_error(invalid_request())),
                });
                let response = self
                    .$method(Box::pin(first.chain(rest)))
                    .await?
                    .into_inner();
                return Ok(Box::pin(
                    response.map(|frame| frame.map(|reply| Reply::$reply(Box::new(reply)))),
                ));
            }};
        }
        macro_rules! unary {
            ($value:expr, $method:ident, $reply:ident) => {{
                if requests.next().await.transpose()?.is_some() {
                    return Err(status_from_error(invalid_request()));
                }
                let value = self.$method(Request::new(*$value)).await?.into_inner();
                Ok(
                    Box::pin(tokio_stream::once(Ok(Reply::$reply(Box::new(value)))))
                        as RpcStream<Reply>,
                )
            }};
        }
        match first {
            Call::ActiveSessionCommand(_)
            | Call::PeerFileRead(_)
            | Call::PeerFileUpdate(_)
            | Call::PeerFileDelete(_)
            | Call::ReadSessionHistory(_)
            | Call::ResolveAttachments(_)
            | Call::ReadMemoryProgress(_)
            | Call::AdvanceMemoryProgress(_)
            | Call::SpawnSubagent(_)
            | Call::SendSubagentMessage(_)
            | Call::WaitSubagent(_)
            | Call::InterruptSubagent(_)
            | Call::FollowUpSubagent(_)
            | Call::ListSubagents(_)
            | Call::SaveToolImage(_) => Err(status_from_error(invalid_request())),
            Call::ListPluginSources(value) => unary!(value, list_plugin_sources, ListPluginSources),
            Call::AddPluginSource(value) => unary!(value, add_plugin_source, AddPluginSource),
            Call::RefreshPluginSource(value) => {
                unary!(value, refresh_plugin_source, RefreshPluginSource)
            }
            Call::RemovePluginSource(value) => {
                unary!(value, remove_plugin_source, RemovePluginSource)
            }
            Call::SearchPlugins(value) => unary!(value, search_plugins, SearchPlugins),
            Call::ListPlugins(value) => unary!(value, list_plugins, ListPlugins),
            Call::ReadPlugin(value) => unary!(value, read_plugin, ReadPlugin),
            Call::InstallPlugin(value) => unary!(value, install_plugin, InstallPlugin),
            Call::UpdatePlugin(value) => unary!(value, update_plugin, UpdatePlugin),
            Call::UninstallPlugin(value) => unary!(value, uninstall_plugin, UninstallPlugin),
            Call::EnablePlugin(value) => unary!(value, enable_plugin, EnablePlugin),
            Call::DisablePlugin(value) => unary!(value, disable_plugin, DisablePlugin),
            Call::ListMcpServers(value) => unary!(value, list_mcp_servers, ListMcpServers),
            Call::ReadMcpAuthentication(value) => {
                unary!(value, read_mcp_authentication, ReadMcpAuthentication)
            }
            Call::UpdateMcpCredentials(value) => {
                unary!(value, update_mcp_credentials, UpdateMcpCredentials)
            }
            Call::ImportMcpServer(value) => unary!(value, import_mcp_server, ImportMcpServer),
            Call::CompleteMcpAuthentication(value) => unary!(
                value,
                complete_mcp_authentication,
                CompleteMcpAuthentication
            ),
            Call::StartMcpAuthentication(value) => {
                unary!(value, start_mcp_authentication, StartMcpAuthentication)
            }

            Call::GetConfig(value) => unary!(value, get_config, GetConfig),
            Call::UpdateConfig(value) => unary!(value, update_config, UpdateConfig),
            Call::DeleteConfig(value) => unary!(value, delete_config, DeleteConfig),
            Call::ReloadConfig(value) => unary!(value, reload_config, ReloadConfig),

            Call::ReadSessionExecutionState(value) => {
                unary!(value, read_session_execution_state, SessionExecutionState)
            }
            Call::CommitSessionExecutionState(value) => {
                unary!(value, commit_session_execution_state, Empty)
            }
            Call::Start(value) => stream!(value, Start, serve_start, Run),
            Call::SubmitInput(value) => stream!(value, SubmitInput, serve_submit_input, Input),
            Call::CompactSession(value) => {
                stream!(value, CompactSession, serve_compact_session, Run)
            }
            Call::FormDreamMemory(value) => {
                stream!(value, FormDreamMemory, serve_dream_memory, DreamMemory)
            }
            Call::Interrupt(value) => unary!(value, interrupt, Interrupt),
            Call::ClearSession(value) => unary!(value, clear_session, Empty),
            Call::CloseSession(value) => unary!(value, close_session, Empty),
            Call::ReadWorkspaceMemory(value) => {
                unary!(value, read_workspace_memory, WorkspaceMemory)
            }
            Call::UpdateWorkspaceMemory(value) => {
                unary!(value, update_workspace_memory, WorkspaceMemory)
            }
            Call::Shutdown(value) => unary!(value, shutdown, Empty),
            Call::ListWorkspaceDocs(value) => unary!(value, list_workspace_docs, ListWorkspaceDocs),
            Call::ReadWorkspaceDoc(value) => unary!(value, read_workspace_doc, ReadWorkspaceDoc),
            Call::WriteWorkspaceDoc(value) => unary!(value, write_workspace_doc, WriteWorkspaceDoc),
            Call::ListMemoryFiles(value) => unary!(value, list_memory_files, ListMemoryFiles),
            Call::ReadMemoryFile(value) => unary!(value, read_memory_file, ReadMemoryFile),
            Call::WriteMemoryFile(value) => unary!(value, write_memory_file, WriteMemoryFile),
            Call::SearchMemory(value) => unary!(value, search_memory, SearchMemory),
            Call::GetMemory(value) => unary!(value, get_memory, GetMemory),
            Call::ListSkills(value) => unary!(value, list_skills, ListSkills),
            Call::RefreshSkills(value) => unary!(value, refresh_skills, RefreshSkills),
            Call::SuggestSkills(value) => unary!(value, suggest_skills, SuggestSkills),
            Call::ReadSkill(value) => unary!(value, read_skill, ReadSkill),
            Call::ResolveSkills(value) => unary!(value, resolve_skills, ResolveSkills),
            Call::WorkspaceInfo(value) => unary!(value, get_workspace_info, WorkspaceInfo),
            Call::ListModels(value) => unary!(value, list_worker_models, ListModels),
            Call::ReadDirectory(value) => unary!(value, read_directory, ReadDirectory),
            Call::CreateDirectory(value) => unary!(value, create_directory, CreateDirectory),
            Call::ProjectStatus(value) => unary!(value, get_project_status, ProjectStatus),
            Call::ValidateProjectDirectory(value) => {
                unary!(value, validate_project_directory, ValidateProjectDirectory)
            }
        }
    }

    /// A reconnect cannot admit work before old committed executions finish cleanup.
    /// Unconfirmed cleanup is retained for this runtime's lifetime and requires host recovery.
    pub async fn drain_disconnected(&self) -> TalosResult<()> {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(error) = self
                    .cleanup_failure
                    .lock()
                    .expect("Agent cleanup lock poisoned")
                    .clone()
                {
                    return Err(error);
                }
                if self
                    .active
                    .lock()
                    .expect("Agent active lock poisoned")
                    .values()
                    .all(|owner| *owner == AuthorizationOwner::Host)
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|error| {
            TalosError::from_source(
                ErrorCode::RunCleanupUnconfirmed,
                "Disconnected Agent cleanup could not be confirmed",
                error,
            )
        })?
    }
}
