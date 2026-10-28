//! Authenticated environment ingress shares the Harness lease and connection lifecycle.
use super::*;
use agent_protocol::{environment_pb as wire, WorkerEnvironment, WorkerEnvironmentContext};
impl AgentHarnessRpcAdapter {
    #[must_use]
    pub fn with_environment(mut self, environment: Arc<dyn WorkerEnvironment>) -> Self {
        self.environment = Some(environment);
        self
    }
    fn environment_context(
        &self,
        auth: Option<pb::NodeAuth>,
        context: Option<pb::WorkerEnvironmentContext>,
    ) -> TalosResult<(Lease, WorkerEnvironmentContext, Arc<dyn WorkerEnvironment>)> {
        let lease = self.authorize(&node_pb::decode_auth(auth)?)?;
        let context = wire::decode_context(context)?;
        let environment = self.environment.clone().ok_or_else(|| {
            TalosError::new(
                ErrorCode::ServiceUnavailable,
                "Worker environment is unavailable",
            )
        })?;
        Ok((lease, context, environment))
    }
}
macro_rules! environment_rpc {
    ($method:ident, $request:ident, $response:ident, $result:ident, $variant:ident, $decode:ident, $encode:ident) => {
        fn $method<'life0, 'async_trait>(&'life0 self, request: Request<pb::$request>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response<pb::$response>, Status>> + Send + 'async_trait>>
        where 'life0: 'async_trait, Self: 'async_trait {
            Box::pin(async move {
            let request = request.into_inner();
            let result = async {
                let (lease, context, environment) = self.environment_context(request.auth, request.context)?;
                let params = wire::$decode(request.params)?;
                let _configuration = match stringify!($method) {
                    "update_config" | "delete_config" | "reload_config"
                    | "add_plugin_source" | "refresh_plugin_source" | "remove_plugin_source"
                    | "install_plugin" | "update_plugin" | "uninstall_plugin" | "enable_plugin" | "disable_plugin"
                    | "update_mcp_credentials" | "import_mcp_server" => Some(self.session_gate.reserve_configuration()?),
                    _ => None,
                };
                let operation = environment.$method(context, params);
                let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
                wire::$encode(value)
            }.await;
            Ok(Response::new(pb::$response { result: Some(match result {
                Ok(value) => pb::$result::Result::$variant(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::$result::Result::Failure(error_pb::encode_failure(&error))
                }
            }) }))
            })
        }
    };
}

#[tonic::async_trait]
impl pb::worker_environment_rpc_server::WorkerEnvironmentRpc for AgentHarnessRpcAdapter {
    environment_rpc!(
        desktop_open,
        DesktopOpenRpcRequest,
        DesktopOpenRpcResponse,
        desktop_open_rpc_response,
        Value,
        decode_desktop_open_params,
        encode_desktop_open_result
    );
    environment_rpc!(
        browser,
        BrowserRpcRequest,
        BrowserRpcResponse,
        browser_rpc_response,
        Value,
        decode_browser_params,
        encode_browser_result
    );
    environment_rpc!(
        list_plugin_sources,
        ListPluginSourcesRequest,
        ListPluginSourcesResponse,
        list_plugin_sources_response,
        Value,
        decode_list_plugin_sources_params,
        encode_list_plugin_sources_result
    );
    environment_rpc!(
        add_plugin_source,
        AddPluginSourceRequest,
        AddPluginSourceResponse,
        add_plugin_source_response,
        Value,
        decode_add_plugin_source_params,
        encode_add_plugin_source_result
    );
    environment_rpc!(
        refresh_plugin_source,
        RefreshPluginSourceRequest,
        RefreshPluginSourceResponse,
        refresh_plugin_source_response,
        Value,
        decode_refresh_plugin_source_params,
        encode_refresh_plugin_source_result
    );
    environment_rpc!(
        remove_plugin_source,
        RemovePluginSourceRequest,
        RemovePluginSourceResponse,
        remove_plugin_source_response,
        Value,
        decode_remove_plugin_source_params,
        encode_remove_plugin_source_result
    );
    environment_rpc!(
        search_plugins,
        SearchPluginsRequest,
        SearchPluginsResponse,
        search_plugins_response,
        Value,
        decode_search_plugins_params,
        encode_search_plugins_result
    );
    environment_rpc!(
        list_plugins,
        ListPluginsRequest,
        ListPluginsResponse,
        list_plugins_response,
        Value,
        decode_list_plugins_params,
        encode_list_plugins_result
    );
    environment_rpc!(
        read_plugin,
        ReadPluginRequest,
        ReadPluginResponse,
        read_plugin_response,
        Value,
        decode_read_plugin_params,
        encode_read_plugin_result
    );
    environment_rpc!(
        prepare_plugin_change,
        PreparePluginChangeRequest,
        PreparePluginChangeResponse,
        prepare_plugin_change_response,
        Value,
        decode_prepare_plugin_change_params,
        encode_prepare_plugin_change_result
    );
    environment_rpc!(
        cancel_plugin_change,
        CancelPluginChangeRequest,
        CancelPluginChangeResponse,
        cancel_plugin_change_response,
        Value,
        decode_cancel_plugin_change_params,
        encode_cancel_plugin_change_result
    );
    environment_rpc!(
        install_plugin,
        InstallPluginRequest,
        InstallPluginResponse,
        install_plugin_response,
        Value,
        decode_worker_install_plugin_params,
        encode_install_plugin_result
    );
    environment_rpc!(
        update_plugin,
        UpdatePluginRequest,
        UpdatePluginResponse,
        update_plugin_response,
        Value,
        decode_worker_update_plugin_params,
        encode_update_plugin_result
    );
    environment_rpc!(
        uninstall_plugin,
        UninstallPluginRequest,
        UninstallPluginResponse,
        uninstall_plugin_response,
        Value,
        decode_uninstall_plugin_params,
        encode_uninstall_plugin_result
    );
    environment_rpc!(
        enable_plugin,
        EnablePluginRequest,
        EnablePluginResponse,
        enable_plugin_response,
        Value,
        decode_enable_plugin_params,
        encode_enable_plugin_result
    );
    environment_rpc!(
        disable_plugin,
        DisablePluginRequest,
        DisablePluginResponse,
        disable_plugin_response,
        Value,
        decode_disable_plugin_params,
        encode_disable_plugin_result
    );
    environment_rpc!(
        list_mcp_servers,
        ListMcpServersRequest,
        ListMcpServersResponse,
        list_mcp_servers_response,
        Value,
        decode_list_mcp_servers_params,
        encode_list_mcp_servers_result
    );
    environment_rpc!(
        read_mcp_authentication,
        ReadMcpAuthenticationRequest,
        ReadMcpAuthenticationResponse,
        read_mcp_authentication_response,
        Value,
        decode_read_mcp_authentication_params,
        encode_read_mcp_authentication_result
    );
    environment_rpc!(
        update_mcp_credentials,
        UpdateMcpCredentialsRequest,
        UpdateMcpCredentialsResponse,
        update_mcp_credentials_response,
        Value,
        decode_update_mcp_credentials_params,
        encode_update_mcp_credentials_result
    );
    environment_rpc!(
        import_mcp_server,
        ImportMcpServerRequest,
        ImportMcpServerResponse,
        import_mcp_server_response,
        Value,
        decode_import_mcp_server_params,
        encode_import_mcp_server_result
    );
    fn start_mcp_authentication<'life0, 'async_trait>(
        &'life0 self,
        request: Request<pb::StartMcpAuthenticationRequest>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<Response<pb::StartMcpAuthenticationResponse>, Status>,
                > + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let request = request.into_inner();
            let result=async {
                let (lease, context, environment)=self.environment_context(request.auth,request.context)?;
                let params=agent_protocol::McpAuthenticationStartParams {
                    request:wire::decode_start_mcp_authentication_params(request.params)?,
                    callback_target:wire::decode_callback_target(request.callback_target)?,
                };
                let value=tokio::select! { biased; _=lease.ended()=>return Err(unauthenticated()), value=environment.start_mcp_authentication(context,params)=>value? };
                wire::encode_start_mcp_authentication_result(value)
            }.await;
            Ok(Response::new(pb::StartMcpAuthenticationResponse {
                result: Some(match result {
                    Ok(value) => pb::start_mcp_authentication_response::Result::Value(value),
                    Err(error) => {
                        talos_logging::report_error(&error);
                        pb::start_mcp_authentication_response::Result::Failure(
                            error_pb::encode_failure(&error),
                        )
                    }
                }),
            }))
        })
    }
    environment_rpc!(
        complete_mcp_authentication,
        CompleteMcpAuthenticationRequest,
        CompleteMcpAuthenticationResponse,
        complete_mcp_authentication_response,
        Value,
        decode_complete_mcp_authentication_params,
        encode_complete_mcp_authentication_result
    );
    environment_rpc!(
        get_config,
        GetConfigRequest,
        GetConfigResponse,
        get_config_response,
        Config,
        decode_get_config_params,
        encode_config_result
    );
    environment_rpc!(
        update_config,
        UpdateConfigRequest,
        UpdateConfigResponse,
        update_config_response,
        Config,
        decode_update_config_params,
        encode_config_result
    );
    environment_rpc!(
        delete_config,
        DeleteConfigRequest,
        DeleteConfigResponse,
        delete_config_response,
        Deleted,
        decode_delete_config_params,
        encode_delete_config_result
    );
    environment_rpc!(
        reload_config,
        ReloadConfigRequest,
        ReloadConfigResponse,
        reload_config_response,
        Reloaded,
        decode_reload_config_params,
        encode_reload_config_result
    );

    environment_rpc!(
        list_memory_files,
        ListMemoryFilesRequest,
        ListMemoryFilesResponse,
        list_memory_files_response,
        Files,
        decode_list_memory_files_params,
        encode_list_memory_files_result
    );
    environment_rpc!(
        read_memory_file,
        ReadMemoryFileRequest,
        ReadMemoryFileResponse,
        read_memory_file_response,
        File,
        decode_read_memory_file_params,
        encode_read_memory_file_result
    );
    environment_rpc!(
        write_memory_file,
        WriteMemoryFileRequest,
        WriteMemoryFileResponse,
        write_memory_file_response,
        File,
        decode_write_memory_file_params,
        encode_write_memory_file_result
    );
    environment_rpc!(
        search_memory,
        SearchMemoryRequest,
        SearchMemoryResponse,
        search_memory_response,
        Hits,
        decode_search_memory_params,
        encode_search_memory_result
    );
    environment_rpc!(
        get_memory,
        GetMemoryRequest,
        GetMemoryResponse,
        get_memory_response,
        Memory,
        decode_get_memory_params,
        encode_get_memory_result
    );
    environment_rpc!(
        list_skills,
        ListSkillsRequest,
        ListSkillsResponse,
        list_skills_response,
        Skills,
        decode_list_skills_params,
        encode_list_skills_result
    );
    environment_rpc!(
        refresh_skills,
        RefreshSkillsRequest,
        RefreshSkillsResponse,
        refresh_skills_response,
        Skills,
        decode_refresh_skills_params,
        encode_refresh_skills_result
    );
    environment_rpc!(
        suggest_skills,
        SuggestSkillsRequest,
        SuggestSkillsResponse,
        suggest_skills_response,
        Suggestions,
        decode_suggest_skills_params,
        encode_suggest_skills_result
    );
    environment_rpc!(
        read_skill,
        ReadSkillRequest,
        ReadSkillResponse,
        read_skill_response,
        Document,
        decode_read_skill_params,
        encode_read_skill_result
    );
    environment_rpc!(
        resolve_skills,
        ResolveSkillsRequest,
        ResolveSkillsResponse,
        resolve_skills_response,
        Skills,
        decode_resolve_skills_params,
        encode_resolve_skills_result
    );

    environment_rpc!(
        list_workspace_docs,
        ListWorkspaceDocsRequest,
        ListWorkspaceDocsResponse,
        list_workspace_docs_response,
        Documents,
        decode_list_workspace_docs_params,
        encode_list_workspace_docs_result
    );
    environment_rpc!(
        read_workspace_doc,
        ReadWorkspaceDocRequest,
        ReadWorkspaceDocResponse,
        read_workspace_doc_response,
        Doc,
        decode_read_workspace_doc_params,
        encode_read_workspace_doc_result
    );
    environment_rpc!(
        write_workspace_doc,
        WriteWorkspaceDocRequest,
        WriteWorkspaceDocResponse,
        write_workspace_doc_response,
        Doc,
        decode_write_workspace_doc_params,
        encode_write_workspace_doc_result
    );

    async fn get_workspace_info(
        &self,
        request: Request<pb::GetWorkspaceInfoRequest>,
    ) -> Result<Response<pb::GetWorkspaceInfoResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;
            request.params.ok_or_else(wire::invalid)?;
            let operation = environment.workspace_info(context);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_workspace(value)
        }.await;
        Ok(Response::new(pb::GetWorkspaceInfoResponse {
            result: Some(match result {
                Ok(value) => pb::get_workspace_info_response::Result::Info(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::get_workspace_info_response::Result::Failure(error_pb::encode_failure(
                        &error,
                    ))
                }
            }),
        }))
    }
    async fn list_worker_models(
        &self,
        request: Request<pb::ListWorkerModelsRequest>,
    ) -> Result<Response<pb::ListWorkerModelsResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;
            request.params.ok_or_else(wire::invalid)?;
            let operation = environment.list_models(context);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_catalog(value)
        }.await;
        Ok(Response::new(pb::ListWorkerModelsResponse {
            result: Some(match result {
                Ok(value) => pb::list_worker_models_response::Result::Catalog(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::list_worker_models_response::Result::Failure(error_pb::encode_failure(
                        &error,
                    ))
                }
            }),
        }))
    }
    environment_rpc!(
        preview_resource,
        PreviewResourceRequest,
        PreviewResourceResponse,
        preview_resource_response,
        Content,
        decode_preview_read,
        encode_preview_content
    );
    async fn read_directory(
        &self,
        request: Request<pb::ReadDirectoryRequest>,
    ) -> Result<Response<pb::ReadDirectoryResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;

            let operation = environment.read_directory(context, wire::decode_read(request.params)?);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_directory(value)
        }.await;
        Ok(Response::new(pb::ReadDirectoryResponse {
            result: Some(match result {
                Ok(value) => pb::read_directory_response::Result::Directory(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::read_directory_response::Result::Failure(error_pb::encode_failure(&error))
                }
            }),
        }))
    }
    async fn create_directory(
        &self,
        request: Request<pb::CreateDirectoryRequest>,
    ) -> Result<Response<pb::CreateDirectoryResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;

            let operation = environment.create_directory(context, wire::decode_create(request.params)?);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_created(value)
        }.await;
        Ok(Response::new(pb::CreateDirectoryResponse {
            result: Some(match result {
                Ok(value) => pb::create_directory_response::Result::Created(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::create_directory_response::Result::Failure(error_pb::encode_failure(&error))
                }
            }),
        }))
    }
    async fn get_project_status(
        &self,
        request: Request<pb::GetProjectStatusRequest>,
    ) -> Result<Response<pb::GetProjectStatusResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;

            let operation = environment.project_status(context, wire::directory(request.params)?);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_status(value)
        }.await;
        Ok(Response::new(pb::GetProjectStatusResponse {
            result: Some(match result {
                Ok(value) => pb::get_project_status_response::Result::Status(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::get_project_status_response::Result::Failure(error_pb::encode_failure(
                        &error,
                    ))
                }
            }),
        }))
    }
    async fn validate_project_directory(
        &self,
        request: Request<pb::ValidateProjectDirectoryRequest>,
    ) -> Result<Response<pb::ValidateProjectDirectoryResponse>, Status> {
        let request = request.into_inner();
        let result = async {
            let (lease, context, environment) = self.environment_context(request.auth, request.context)?;

            let operation = environment.validate_project_directory(context, wire::directory(request.params)?);
            let value = tokio::select! { biased; _ = lease.ended() => return Err(unauthenticated()), value = operation => value? };
            wire::encode_validated(value)
        }.await;
        Ok(Response::new(pb::ValidateProjectDirectoryResponse {
            result: Some(match result {
                Ok(value) => pb::validate_project_directory_response::Result::Directory(value),
                Err(error) => {
                    talos_logging::report_error(&error);
                    pb::validate_project_directory_response::Result::Failure(
                        error_pb::encode_failure(&error),
                    )
                }
            }),
        }))
    }
}
