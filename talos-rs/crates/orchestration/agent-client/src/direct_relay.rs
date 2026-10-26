//! Direct publication relays the original typed RPC without a second Worker gate.
use super::*;
use crate::transport_stream::RpcStream;
use futures_util::{Stream, StreamExt};
use pb::worker_rpc_call::Request as Call;
use pb::worker_rpc_result::Response as Reply;
use std::{pin::Pin, sync::Mutex};
use tonic::Status;

impl AgentEnvironmentClient {
    pub(crate) fn forward(
        self: &Arc<Self>,
        requests: Pin<Box<dyn Stream<Item = Result<Call, Status>> + Send>>,
        timeout_ms: u64,
        caller: Option<pb::CoordinatorCallContext>,
    ) -> futures_util::future::BoxFuture<'static, Result<RpcStream<Reply>, Status>> {
        if self.is_closed() {
            return Box::pin(async {
                Err(error_pb::encode_status(&crate::connection_writer::closed()))
            });
        }
        let channel = match &self.transport {
            HarnessTransport::Direct(channel) => channel.clone(),
            HarnessTransport::Outbound(connection) => {
                let connection = connection.clone();
                return Box::pin(
                    async move { connection.forward(requests, timeout_ms, caller).await },
                );
            }
        };
        let auth = node_pb::encode_auth(self.auth.clone());
        let mut requests: Pin<Box<dyn Stream<Item = Result<Call, Status>> + Send>> =
            Box::pin(requests.map(move |request| {
                request.map(|mut request| {
                    agent_protocol::connection_pb::replace_auth(&mut request, &auth);
                    request
                })
            }));
        Box::pin(async move {
            let mut first = requests.next().await.transpose()?.ok_or_else(invalid)?;
            if let Some(caller) = &caller {
                agent_protocol::connection_pb::set_execution_caller(&mut first, caller);
            }
            let mut harness =
                pb::agent_harness_rpc_client::AgentHarnessRpcClient::new(channel.clone());
            let mut environment =
                pb::worker_environment_rpc_client::WorkerEnvironmentRpcClient::new(channel.clone());
            macro_rules! stream {
                ($value:expr, $variant:ident, $method:ident, $reply:ident) => {{
                    // Tonic request streams carry messages only. Preserve a malformed
                    // carrier's original failure when half-closing the gRPC request.
                    let failure = Arc::new(Mutex::new(None));
                    let input_failure = failure.clone();
                    let (sender, input) = tokio::sync::mpsc::channel(crate::connection_writer::CALL_CAPACITY);
                    sender.send(*$value).await.map_err(|_| invalid())?;
                    tokio::spawn(async move {
                        loop {
                            let frame = tokio::select! {
                                _ = sender.closed() => break,
                                frame = requests.next() => match frame { Some(frame) => frame, None => break },
                            };
                            let value = match frame {
                                Ok(Call::$variant(value)) => *value,
                                frame => {
                                    *input_failure.lock().expect("Direct relay failure lock") =
                                        Some(frame.err().unwrap_or_else(invalid));
                                    break;
                                }
                            };
                            if sender.send(value).await.is_err() { break; }
                        }
                    });
                    let mut request = Request::new(tokio_stream::wrappers::ReceiverStream::new(input));
                    if timeout_ms > 0 { request.set_timeout(Duration::from_millis(timeout_ms)); }
                    let response = harness.$method(request)
                        .await
                        .map_err(|error| {
                            failure
                                .lock()
                                .expect("Direct relay failure lock")
                                .take()
                                .unwrap_or(error)
                        })?
                        .into_inner();
                    let output_failure = failure.clone();
                    let response = response.map(move |frame| {
                        if let Some(error) = output_failure
                            .lock()
                            .expect("Direct relay failure lock")
                            .take()
                        {
                            return Err(error);
                        }
                        frame.map(|value| Reply::$reply(Box::new(value)))
                    });
                    let last = futures_util::stream::once(async move {
                        failure
                            .lock()
                            .expect("Direct relay failure lock")
                            .take()
                            .map(Err)
                    })
                    .filter_map(|item| async move { item });
                    return Ok(RpcStream::new(response.chain(last)));
                }};
            }
            macro_rules! unary {
                ($value:expr, $client:ident, $method:ident, $reply:ident) => {{
                    if requests.next().await.transpose()?.is_some() {
                        return Err(invalid());
                    }
                    let mut request = Request::new(*$value);
                    if timeout_ms > 0 {
                        request.set_timeout(Duration::from_millis(timeout_ms));
                    }
                    let response = $client.$method(request).await?.into_inner();
                    Ok(RpcStream::new(tokio_stream::once(Ok(Reply::$reply(
                        Box::new(response),
                    )))))
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
                | Call::SaveToolImage(_) => Err(invalid()),
                Call::ListPluginSources(value) => {
                    unary!(value, environment, list_plugin_sources, ListPluginSources)
                }
                Call::AddPluginSource(value) => {
                    unary!(value, environment, add_plugin_source, AddPluginSource)
                }
                Call::RefreshPluginSource(value) => {
                    unary!(
                        value,
                        environment,
                        refresh_plugin_source,
                        RefreshPluginSource
                    )
                }
                Call::RemovePluginSource(value) => {
                    unary!(value, environment, remove_plugin_source, RemovePluginSource)
                }
                Call::SearchPlugins(value) => {
                    unary!(value, environment, search_plugins, SearchPlugins)
                }
                Call::ListPlugins(value) => unary!(value, environment, list_plugins, ListPlugins),
                Call::ReadPlugin(value) => unary!(value, environment, read_plugin, ReadPlugin),
                Call::PreparePluginChange(value) => {
                    unary!(
                        value,
                        environment,
                        prepare_plugin_change,
                        PreparePluginChange
                    )
                }
                Call::CancelPluginChange(value) => {
                    unary!(value, environment, cancel_plugin_change, CancelPluginChange)
                }
                Call::InstallPlugin(value) => {
                    unary!(value, environment, install_plugin, InstallPlugin)
                }
                Call::UpdatePlugin(value) => {
                    unary!(value, environment, update_plugin, UpdatePlugin)
                }
                Call::UninstallPlugin(value) => {
                    unary!(value, environment, uninstall_plugin, UninstallPlugin)
                }
                Call::EnablePlugin(value) => {
                    unary!(value, environment, enable_plugin, EnablePlugin)
                }
                Call::DisablePlugin(value) => {
                    unary!(value, environment, disable_plugin, DisablePlugin)
                }
                Call::DesktopOpen(value) => unary!(value, environment, desktop_open, DesktopOpen),
                Call::Browser(value) => unary!(value, environment, browser, Browser),
                Call::ListMcpServers(value) => {
                    unary!(value, environment, list_mcp_servers, ListMcpServers)
                }
                Call::ReadMcpAuthentication(value) => {
                    unary!(
                        value,
                        environment,
                        read_mcp_authentication,
                        ReadMcpAuthentication
                    )
                }
                Call::UpdateMcpCredentials(value) => {
                    unary!(
                        value,
                        environment,
                        update_mcp_credentials,
                        UpdateMcpCredentials
                    )
                }
                Call::ImportMcpServer(value) => {
                    unary!(value, environment, import_mcp_server, ImportMcpServer)
                }
                Call::CompleteMcpAuthentication(value) => unary!(
                    value,
                    environment,
                    complete_mcp_authentication,
                    CompleteMcpAuthentication
                ),
                Call::StartMcpAuthentication(value) => {
                    unary!(
                        value,
                        environment,
                        start_mcp_authentication,
                        StartMcpAuthentication
                    )
                }

                Call::GetConfig(value) => unary!(value, environment, get_config, GetConfig),
                Call::UpdateConfig(value) => {
                    unary!(value, environment, update_config, UpdateConfig)
                }
                Call::DeleteConfig(value) => {
                    unary!(value, environment, delete_config, DeleteConfig)
                }
                Call::ReloadConfig(value) => {
                    unary!(value, environment, reload_config, ReloadConfig)
                }

                Call::SetRegistrationAdmission(_) => Err(invalid()),
                Call::ReadSessionExecutionState(value) => {
                    unary!(
                        value,
                        harness,
                        read_session_execution_state,
                        SessionExecutionState
                    )
                }
                Call::CommitSessionExecutionState(value) => {
                    unary!(value, harness, commit_session_execution_state, Empty)
                }
                Call::Start(value) => stream!(value, Start, start, Run),
                Call::SubmitInput(value) => stream!(value, SubmitInput, submit_input, Input),
                Call::CompactSession(value) => {
                    stream!(value, CompactSession, compact_session, Run)
                }
                Call::FormDreamMemory(value) => {
                    stream!(value, FormDreamMemory, form_dream_memory, DreamMemory)
                }
                Call::Interrupt(value) => unary!(value, harness, interrupt, Interrupt),
                Call::ClearSession(value) => unary!(value, harness, clear_session, Empty),
                Call::CloseSession(value) => unary!(value, harness, close_session, Empty),
                Call::ReadWorkspaceMemory(value) => {
                    unary!(value, harness, read_workspace_memory, WorkspaceMemory)
                }
                Call::UpdateWorkspaceMemory(value) => {
                    unary!(value, harness, update_workspace_memory, WorkspaceMemory)
                }
                Call::Shutdown(value) => unary!(value, harness, shutdown, Empty),
                Call::ListWorkspaceDocs(value) => {
                    unary!(value, environment, list_workspace_docs, ListWorkspaceDocs)
                }
                Call::ReadWorkspaceDoc(value) => {
                    unary!(value, environment, read_workspace_doc, ReadWorkspaceDoc)
                }
                Call::WriteWorkspaceDoc(value) => {
                    unary!(value, environment, write_workspace_doc, WriteWorkspaceDoc)
                }
                Call::ListMemoryFiles(value) => {
                    unary!(value, environment, list_memory_files, ListMemoryFiles)
                }
                Call::ReadMemoryFile(value) => {
                    unary!(value, environment, read_memory_file, ReadMemoryFile)
                }
                Call::WriteMemoryFile(value) => {
                    unary!(value, environment, write_memory_file, WriteMemoryFile)
                }
                Call::SearchMemory(value) => {
                    unary!(value, environment, search_memory, SearchMemory)
                }
                Call::GetMemory(value) => unary!(value, environment, get_memory, GetMemory),
                Call::ListSkills(value) => unary!(value, environment, list_skills, ListSkills),
                Call::RefreshSkills(value) => {
                    unary!(value, environment, refresh_skills, RefreshSkills)
                }
                Call::SuggestSkills(value) => {
                    unary!(value, environment, suggest_skills, SuggestSkills)
                }
                Call::ReadSkill(value) => unary!(value, environment, read_skill, ReadSkill),
                Call::ResolveSkills(value) => {
                    unary!(value, environment, resolve_skills, ResolveSkills)
                }
                Call::WorkspaceInfo(value) => {
                    unary!(value, environment, get_workspace_info, WorkspaceInfo)
                }
                Call::ListModels(value) => {
                    unary!(value, environment, list_worker_models, ListModels)
                }
                Call::ReadDirectory(value) => {
                    unary!(value, environment, read_directory, ReadDirectory)
                }
                Call::CreateDirectory(value) => {
                    unary!(value, environment, create_directory, CreateDirectory)
                }
                Call::ProjectStatus(value) => {
                    unary!(value, environment, get_project_status, ProjectStatus)
                }
                Call::ValidateProjectDirectory(value) => {
                    unary!(
                        value,
                        environment,
                        validate_project_directory,
                        ValidateProjectDirectory
                    )
                }
            }
        })
    }
}
fn invalid() -> Status {
    error_pb::encode_status(&TalosError::new(
        ErrorCode::InvalidRequest,
        "Worker RPC carrier is invalid",
    ))
}
