//! One environment SDK hides direct gRPC and reverse Connect behind the domain port.
use crate::{
    connection::RequestMode, harness_transport::HarnessTransport, AgentExecutionConnection,
    AgentTlsTrust,
};
use agent_pb as pb;
use agent_protocol::{
    environment_pb as wire, error_pb, node_pb, AgentFuture, NodeAuth, WorkerEnvironment,
    WorkerEnvironmentContext, WorkerPluginChange, WorkerPluginInstall, WorkerPluginPrepared,
    WorkerPluginUpdate,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::*;
use tonic::{transport::Endpoint, Request};
#[derive(Clone)]
pub struct AgentEnvironmentClient {
    transport: HarnessTransport,
    auth: NodeAuth,
    closed: tokio_util::sync::CancellationToken,
}
#[path = "direct_relay.rs"]
mod direct_relay;

impl AgentEnvironmentClient {
    pub async fn connect_loopback(
        address: std::net::SocketAddr,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        if auth.node_id.is_empty() || auth.token.is_empty() {
            return Err(wire::invalid());
        }
        Ok(Self {
            transport: HarnessTransport::Direct(crate::tls::loopback_channel(address).await?),
            auth,
            closed: tokio_util::sync::CancellationToken::new(),
        })
    }

    pub async fn connect(
        endpoint: &str,
        trust: AgentTlsTrust,
        auth: NodeAuth,
    ) -> TalosResult<Self> {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        if !endpoint.starts_with("https://") || auth.node_id.is_empty() || auth.token.is_empty() {
            return Err(wire::invalid());
        }
        let channel = Endpoint::from_shared(endpoint.to_owned())
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::InvalidRequest,
                    "Worker endpoint is invalid",
                    error,
                )
            })?
            .connect_timeout(Duration::from_secs(10))
            .tls_config(trust.into_config())
            .map_err(|error| {
                TalosError::internal_with_source(
                    "Worker TLS configuration failed; technical details were withheld.",
                    error,
                )
            })?
            .connect()
            .await
            .map_err(|error| {
                TalosError::from_source(
                    ErrorCode::ServiceUnavailable,
                    "Worker RPC is unavailable",
                    error,
                )
            })?;
        Ok(Self {
            transport: HarnessTransport::Direct(channel),
            auth,
            closed: tokio_util::sync::CancellationToken::new(),
        })
    }
    pub fn from_connection(connection: Arc<AgentExecutionConnection>, auth: NodeAuth) -> Self {
        Self {
            transport: HarnessTransport::Outbound(connection),
            auth,
            closed: tokio_util::sync::CancellationToken::new(),
        }
    }
    /// Reserve idle Worker admission during a registration deletion; local host access remains independent.
    pub async fn set_registration_admission(&self, blocked: bool) -> TalosResult<()> {
        let request = pb::SetRegistrationAdmissionRequest {
            auth: Some(node_pb::encode_auth(self.auth.clone())),
            blocked,
        };
        let mut request = Request::new(request);
        request.set_timeout(Duration::from_secs(10));
        let response = self
            .transport
            .set_registration_admission(request)
            .await
            .map_err(crate::decode_rpc_status)?
            .into_inner();
        match response.result.ok_or_else(wire::invalid)? {
            pb::empty_response::Result::Completed(_) => Ok(()),
            pb::empty_response::Result::Failure(error) => Err(error_pb::decode_failure(error)?),
        }
    }

    /// Withdraw this published transport and interrupt its active relays.
    pub fn close(&self) {
        self.closed.cancel();
        if let HarnessTransport::Outbound(connection) = &self.transport {
            connection.close();
        }
    }
    /// A closed publication cannot be reused after lease revocation.
    pub fn is_closed(&self) -> bool {
        self.closed.is_cancelled()
            || matches!(&self.transport, HarnessTransport::Outbound(connection) if connection.is_closed())
    }
    /// Wait for publication withdrawal or an outbound connection failure.
    pub async fn ended(&self) {
        tokio::select! {
            _ = self.closed.cancelled() => {}
            _ = async {
                match &self.transport {
                    HarnessTransport::Outbound(connection) => connection.ended().await,
                    HarnessTransport::Direct(_) => futures_util::future::pending().await,
                }
            } => {}
        }
    }
}
macro_rules! invoke {
    ($client:expr, $method:ident, $call:ident, $request:expr, $result:ident, $variant:ident, $decode:ident) => {{
        if $client.is_closed() {
            return Err(crate::connection_writer::closed());
        }
        let mut request = Request::new($request);
        let timeout = match stringify!($method) {
            "add_plugin_source"
            | "refresh_plugin_source"
            | "search_plugins"
            | "start_mcp_authentication"
            | "complete_mcp_authentication" => 60,
            "install_plugin" | "update_plugin" => 300,
            "prepare_plugin_change" => 600,
            _ => 10,
        };
        request.set_timeout(Duration::from_secs(timeout));
        let response = match &$client.transport {
            HarnessTransport::Direct(channel) => {
                pb::worker_environment_rpc_client::WorkerEnvironmentRpcClient::new(channel.clone())
                    .$method(request)
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .into_inner()
            }
            HarnessTransport::Outbound(connection) => {
                let mut stream = connection
                    .invoke(
                        tokio_stream::once(pb::worker_rpc_call::Request::$call(Box::new(
                            request.into_inner(),
                        ))),
                        timeout * 1_000,
                        RequestMode::Unary,
                    )
                    .await
                    .map_err(crate::decode_rpc_status)?;
                let reply = stream
                    .message()
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .ok_or_else(wire::invalid)?;
                let pb::worker_rpc_result::Response::$call(response) = reply else {
                    return Err(wire::invalid());
                };
                if stream
                    .message()
                    .await
                    .map_err(crate::decode_rpc_status)?
                    .is_some()
                {
                    return Err(wire::invalid());
                }
                *response
            }
        };
        match response.result.ok_or_else(wire::invalid)? {
            pb::$result::Result::$variant(value) => wire::$decode(value),
            pb::$result::Result::Failure(failure) => Err(error_pb::decode_failure(failure)?),
        }
    }};
}
macro_rules! environment_method {
    ($method:ident, $params:ty, $response:ty, $request:ident, $call:ident, $result:ident, $variant:ident, $encode:ident, $decode:ident) => {
        fn $method(
            &self,
            context: WorkerEnvironmentContext,
            params: $params,
        ) -> AgentFuture<'_, TalosResult<$response>> {
            Box::pin(async move {
                invoke!(
                    self,
                    $method,
                    $call,
                    pb::$request {
                        auth: Some(node_pb::encode_auth(self.auth.clone())),
                        context: Some(wire::encode_context(context)),
                        params: Some(wire::$encode(params)?),
                    },
                    $result,
                    $variant,
                    $decode
                )
            })
        }
    };
}
impl WorkerEnvironment for AgentEnvironmentClient {
    environment_method!(
        desktop_open,
        WorkerDesktopOpen,
        (),
        DesktopOpenRpcRequest,
        DesktopOpen,
        desktop_open_rpc_response,
        Value,
        encode_desktop_open_params,
        decode_desktop_open_result
    );
    environment_method!(
        browser,
        BrowserRequest,
        BrowserSnapshot,
        BrowserRpcRequest,
        Browser,
        browser_rpc_response,
        Value,
        encode_browser_params,
        decode_browser_result
    );
    environment_method!(
        list_plugin_sources,
        PluginSourceListRequest,
        PluginSourceListResponse,
        ListPluginSourcesRequest,
        ListPluginSources,
        list_plugin_sources_response,
        Value,
        encode_list_plugin_sources_params,
        decode_list_plugin_sources_result
    );
    environment_method!(
        add_plugin_source,
        PluginSourceAddRequest,
        PluginSourceAddResponse,
        AddPluginSourceRequest,
        AddPluginSource,
        add_plugin_source_response,
        Value,
        encode_add_plugin_source_params,
        decode_add_plugin_source_result
    );
    environment_method!(
        refresh_plugin_source,
        PluginSourceRefreshRequest,
        PluginSourceRefreshResponse,
        RefreshPluginSourceRequest,
        RefreshPluginSource,
        refresh_plugin_source_response,
        Value,
        encode_refresh_plugin_source_params,
        decode_refresh_plugin_source_result
    );
    environment_method!(
        remove_plugin_source,
        PluginSourceRemoveRequest,
        PluginSourceRemoveResponse,
        RemovePluginSourceRequest,
        RemovePluginSource,
        remove_plugin_source_response,
        Value,
        encode_remove_plugin_source_params,
        decode_remove_plugin_source_result
    );
    environment_method!(
        search_plugins,
        PluginSearchRequest,
        PluginSearchResponse,
        SearchPluginsRequest,
        SearchPlugins,
        search_plugins_response,
        Value,
        encode_search_plugins_params,
        decode_search_plugins_result
    );
    environment_method!(
        list_plugins,
        PluginListRequest,
        PluginListResponse,
        ListPluginsRequest,
        ListPlugins,
        list_plugins_response,
        Value,
        encode_list_plugins_params,
        decode_list_plugins_result
    );
    environment_method!(
        read_plugin,
        PluginReadRequest,
        PluginReadResponse,
        ReadPluginRequest,
        ReadPlugin,
        read_plugin_response,
        Value,
        encode_read_plugin_params,
        decode_read_plugin_result
    );
    environment_method!(
        prepare_plugin_change,
        WorkerPluginChange,
        WorkerPluginPrepared,
        PreparePluginChangeRequest,
        PreparePluginChange,
        prepare_plugin_change_response,
        Value,
        encode_prepare_plugin_change_params,
        decode_prepare_plugin_change_result
    );
    environment_method!(
        install_plugin,
        WorkerPluginInstall,
        PluginInstallResponse,
        InstallPluginRequest,
        InstallPlugin,
        install_plugin_response,
        Value,
        encode_worker_install_plugin_params,
        decode_install_plugin_result
    );
    environment_method!(
        update_plugin,
        WorkerPluginUpdate,
        PluginUpdateResponse,
        UpdatePluginRequest,
        UpdatePlugin,
        update_plugin_response,
        Value,
        encode_worker_update_plugin_params,
        decode_update_plugin_result
    );
    environment_method!(
        uninstall_plugin,
        PluginUninstallRequest,
        PluginUninstallResponse,
        UninstallPluginRequest,
        UninstallPlugin,
        uninstall_plugin_response,
        Value,
        encode_uninstall_plugin_params,
        decode_uninstall_plugin_result
    );
    environment_method!(
        enable_plugin,
        PluginEnableRequest,
        PluginEnableResponse,
        EnablePluginRequest,
        EnablePlugin,
        enable_plugin_response,
        Value,
        encode_enable_plugin_params,
        decode_enable_plugin_result
    );
    environment_method!(
        disable_plugin,
        PluginDisableRequest,
        PluginDisableResponse,
        DisablePluginRequest,
        DisablePlugin,
        disable_plugin_response,
        Value,
        encode_disable_plugin_params,
        decode_disable_plugin_result
    );
    environment_method!(
        list_mcp_servers,
        McpServerListRequest,
        McpServerListResponse,
        ListMcpServersRequest,
        ListMcpServers,
        list_mcp_servers_response,
        Value,
        encode_list_mcp_servers_params,
        decode_list_mcp_servers_result
    );
    environment_method!(
        read_mcp_authentication,
        McpAuthenticationReadRequest,
        McpAuthenticationResponse,
        ReadMcpAuthenticationRequest,
        ReadMcpAuthentication,
        read_mcp_authentication_response,
        Value,
        encode_read_mcp_authentication_params,
        decode_read_mcp_authentication_result
    );
    environment_method!(
        update_mcp_credentials,
        McpCredentialsUpdateRequest,
        McpAuthenticationResponse,
        UpdateMcpCredentialsRequest,
        UpdateMcpCredentials,
        update_mcp_credentials_response,
        Value,
        encode_update_mcp_credentials_params,
        decode_update_mcp_credentials_result
    );
    environment_method!(
        import_mcp_server,
        McpServerImportRequest,
        PluginInstallResponse,
        ImportMcpServerRequest,
        ImportMcpServer,
        import_mcp_server_response,
        Value,
        encode_import_mcp_server_params,
        decode_import_mcp_server_result
    );
    fn start_mcp_authentication(
        &self,
        context: WorkerEnvironmentContext,
        params: agent_protocol::McpAuthenticationStartParams,
    ) -> AgentFuture<'_, TalosResult<McpAuthenticationStartResponse>> {
        Box::pin(async move {
            invoke!(
                self,
                start_mcp_authentication,
                StartMcpAuthentication,
                pb::StartMcpAuthenticationRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(wire::encode_start_mcp_authentication_params(
                        params.request
                    )?),
                    callback_target: Some(wire::encode_callback_target(params.callback_target)),
                },
                start_mcp_authentication_response,
                Value,
                decode_start_mcp_authentication_result
            )
        })
    }
    environment_method!(
        complete_mcp_authentication,
        agent_protocol::McpAuthenticationCallbackParams,
        McpAuthenticationResponse,
        CompleteMcpAuthenticationRequest,
        CompleteMcpAuthentication,
        complete_mcp_authentication_response,
        Value,
        encode_complete_mcp_authentication_params,
        decode_complete_mcp_authentication_result
    );
    environment_method!(
        get_config,
        ConfigGetRequest,
        ConfigResponse,
        GetConfigRequest,
        GetConfig,
        get_config_response,
        Config,
        encode_get_config_params,
        decode_config_result
    );
    environment_method!(
        update_config,
        ConfigUpdateRequest,
        ConfigResponse,
        UpdateConfigRequest,
        UpdateConfig,
        update_config_response,
        Config,
        encode_update_config_params,
        decode_config_result
    );
    environment_method!(
        delete_config,
        ConfigDeleteRequest,
        ConfigDeleteResponse,
        DeleteConfigRequest,
        DeleteConfig,
        delete_config_response,
        Deleted,
        encode_delete_config_params,
        decode_delete_config_result
    );
    environment_method!(
        reload_config,
        ConfigReloadRequest,
        ConfigReloadResponse,
        ReloadConfigRequest,
        ReloadConfig,
        reload_config_response,
        Reloaded,
        encode_reload_config_params,
        decode_reload_config_result
    );

    environment_method!(
        list_memory_files,
        MemoryFileListRequest,
        MemoryFileListResponse,
        ListMemoryFilesRequest,
        ListMemoryFiles,
        list_memory_files_response,
        Files,
        encode_list_memory_files_params,
        decode_list_memory_files_result
    );
    environment_method!(
        read_memory_file,
        MemoryFileReadRequest,
        MemoryFileReadResponse,
        ReadMemoryFileRequest,
        ReadMemoryFile,
        read_memory_file_response,
        File,
        encode_read_memory_file_params,
        decode_read_memory_file_result
    );
    environment_method!(
        write_memory_file,
        MemoryFileWriteRequest,
        MemoryFileWriteResponse,
        WriteMemoryFileRequest,
        WriteMemoryFile,
        write_memory_file_response,
        File,
        encode_write_memory_file_params,
        decode_write_memory_file_result
    );
    environment_method!(
        search_memory,
        MemorySearchRequest,
        MemorySearchResponse,
        SearchMemoryRequest,
        SearchMemory,
        search_memory_response,
        Hits,
        encode_search_memory_params,
        decode_search_memory_result
    );
    environment_method!(
        get_memory,
        MemoryGetRequest,
        MemoryGetResponse,
        GetMemoryRequest,
        GetMemory,
        get_memory_response,
        Memory,
        encode_get_memory_params,
        decode_get_memory_result
    );
    environment_method!(
        list_skills,
        SkillsListRequest,
        SkillsListResponse,
        ListSkillsRequest,
        ListSkills,
        list_skills_response,
        Skills,
        encode_list_skills_params,
        decode_list_skills_result
    );
    environment_method!(
        refresh_skills,
        SkillsRefreshRequest,
        SkillsListResponse,
        RefreshSkillsRequest,
        RefreshSkills,
        refresh_skills_response,
        Skills,
        encode_refresh_skills_params,
        decode_refresh_skills_result
    );
    environment_method!(
        suggest_skills,
        SkillsSuggestRequest,
        SkillsSuggestResponse,
        SuggestSkillsRequest,
        SuggestSkills,
        suggest_skills_response,
        Suggestions,
        encode_suggest_skills_params,
        decode_suggest_skills_result
    );
    environment_method!(
        read_skill,
        SkillsReadRequest,
        SkillsReadResponse,
        ReadSkillRequest,
        ReadSkill,
        read_skill_response,
        Document,
        encode_read_skill_params,
        decode_read_skill_result
    );
    environment_method!(
        resolve_skills,
        SkillsResolveRequest,
        SkillsResolveResponse,
        ResolveSkillsRequest,
        ResolveSkills,
        resolve_skills_response,
        Skills,
        encode_resolve_skills_params,
        decode_resolve_skills_result
    );

    environment_method!(
        list_workspace_docs,
        WorkspaceDocListRequest,
        WorkspaceDocListResponse,
        ListWorkspaceDocsRequest,
        ListWorkspaceDocs,
        list_workspace_docs_response,
        Documents,
        encode_list_workspace_docs_params,
        decode_list_workspace_docs_result
    );
    environment_method!(
        read_workspace_doc,
        WorkspaceDocReadRequest,
        WorkspaceDocReadResponse,
        ReadWorkspaceDocRequest,
        ReadWorkspaceDoc,
        read_workspace_doc_response,
        Doc,
        encode_read_workspace_doc_params,
        decode_read_workspace_doc_result
    );
    environment_method!(
        write_workspace_doc,
        WorkspaceDocWriteRequest,
        WorkspaceDocWriteResponse,
        WriteWorkspaceDocRequest,
        WriteWorkspaceDoc,
        write_workspace_doc_response,
        Doc,
        encode_write_workspace_doc_params,
        decode_write_workspace_doc_result
    );

    fn workspace_info(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, TalosResult<WorkspaceInfoResponse>> {
        Box::pin(async move {
            invoke!(
                self,
                get_workspace_info,
                WorkspaceInfo,
                pb::GetWorkspaceInfoRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(pb::WorkspaceInfoParams {})
                },
                get_workspace_info_response,
                Info,
                decode_workspace
            )
        })
    }
    fn list_models(
        &self,
        context: WorkerEnvironmentContext,
    ) -> AgentFuture<'_, TalosResult<ModelListResponse>> {
        Box::pin(async move {
            invoke!(
                self,
                list_worker_models,
                ListModels,
                pb::ListWorkerModelsRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(pb::ModelListParams {})
                },
                list_worker_models_response,
                Catalog,
                decode_catalog
            )
        })
    }
    fn read_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryReadRequest,
    ) -> AgentFuture<'_, TalosResult<FsDirectorySnapshot>> {
        Box::pin(async move {
            invoke!(
                self,
                read_directory,
                ReadDirectory,
                pb::ReadDirectoryRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(wire::encode_read(params)?)
                },
                read_directory_response,
                Directory,
                decode_directory
            )
        })
    }
    fn create_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: FsDirectoryCreateRequest,
    ) -> AgentFuture<'_, TalosResult<FsDirectoryCreateResponse>> {
        Box::pin(async move {
            invoke!(
                self,
                create_directory,
                CreateDirectory,
                pb::CreateDirectoryRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(wire::encode_create(params)?)
                },
                create_directory_response,
                Created,
                decode_created
            )
        })
    }
    fn project_status(
        &self,
        context: WorkerEnvironmentContext,
        params: PathBuf,
    ) -> AgentFuture<'_, TalosResult<ProjectStatus>> {
        Box::pin(async move {
            invoke!(
                self,
                get_project_status,
                ProjectStatus,
                pb::GetProjectStatusRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(pb::ProjectDirectoryParams {
                        directory: wire::path(params)?
                    })
                },
                get_project_status_response,
                Status,
                decode_status
            )
        })
    }
    fn validate_project_directory(
        &self,
        context: WorkerEnvironmentContext,
        params: PathBuf,
    ) -> AgentFuture<'_, TalosResult<PathBuf>> {
        Box::pin(async move {
            invoke!(
                self,
                validate_project_directory,
                ValidateProjectDirectory,
                pb::ValidateProjectDirectoryRequest {
                    auth: Some(node_pb::encode_auth(self.auth.clone())),
                    context: Some(wire::encode_context(context)),
                    params: Some(pb::ProjectDirectoryParams {
                        directory: wire::path(params)?
                    })
                },
                validate_project_directory_response,
                Directory,
                decode_validated
            )
        })
    }
}
