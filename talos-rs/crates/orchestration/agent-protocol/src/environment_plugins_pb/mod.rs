//! Explicit projections of canonical Plugin and MCP objects.
use crate::environment_pb::invalid;
use agent_pb as pb;
use talos_error::TalosResult;
use talos_types as d;
mod installations;
use installations::*;
mod metadata;
use metadata::*;
mod sources;
use sources::*;
mod mcp_authentication;
use mcp_authentication::*;
mod mcp_authentication_status;
use mcp_authentication_status::*;
mod mcp_credentials;
use mcp_credentials::*;
mod mcp_servers;
use mcp_servers::*;
mod definitions;
use definitions::*;
mod status;
use status::*;
mod install_operations;
use install_operations::*;
mod catalog_operations;
use catalog_operations::*;
mod source_operations;
use source_operations::*;
pub fn encode_list_plugin_sources_params(
    value: d::PluginSourceListRequest,
) -> TalosResult<pb::PluginSourceListParams> {
    encode_plugin_source_list_request(value)
}
pub fn decode_list_plugin_sources_params(
    value: Option<pb::PluginSourceListParams>,
) -> TalosResult<d::PluginSourceListRequest> {
    decode_plugin_source_list_request(value.ok_or_else(invalid)?)
}
pub fn encode_list_plugin_sources_result(
    value: d::PluginSourceListResponse,
) -> TalosResult<pb::PluginSourceListResult> {
    encode_plugin_source_list_response(value)
}
pub fn decode_list_plugin_sources_result(
    value: pb::PluginSourceListResult,
) -> TalosResult<d::PluginSourceListResponse> {
    decode_plugin_source_list_response(value)
}
pub fn encode_add_plugin_source_params(
    value: d::PluginSourceAddRequest,
) -> TalosResult<pb::PluginSourceAddParams> {
    encode_plugin_source_add_request(value)
}
pub fn decode_add_plugin_source_params(
    value: Option<pb::PluginSourceAddParams>,
) -> TalosResult<d::PluginSourceAddRequest> {
    decode_plugin_source_add_request(value.ok_or_else(invalid)?)
}
pub fn encode_add_plugin_source_result(
    value: d::PluginSourceAddResponse,
) -> TalosResult<pb::PluginSourceAddResult> {
    encode_plugin_source_add_response(value)
}
pub fn decode_add_plugin_source_result(
    value: pb::PluginSourceAddResult,
) -> TalosResult<d::PluginSourceAddResponse> {
    decode_plugin_source_add_response(value)
}
pub fn encode_refresh_plugin_source_params(
    value: d::PluginSourceRefreshRequest,
) -> TalosResult<pb::PluginSourceRefreshParams> {
    encode_plugin_source_refresh_request(value)
}
pub fn decode_refresh_plugin_source_params(
    value: Option<pb::PluginSourceRefreshParams>,
) -> TalosResult<d::PluginSourceRefreshRequest> {
    decode_plugin_source_refresh_request(value.ok_or_else(invalid)?)
}
pub fn encode_refresh_plugin_source_result(
    value: d::PluginSourceRefreshResponse,
) -> TalosResult<pb::PluginSourceRefreshResult> {
    encode_plugin_source_refresh_response(value)
}
pub fn decode_refresh_plugin_source_result(
    value: pb::PluginSourceRefreshResult,
) -> TalosResult<d::PluginSourceRefreshResponse> {
    decode_plugin_source_refresh_response(value)
}
pub fn encode_remove_plugin_source_params(
    value: d::PluginSourceRemoveRequest,
) -> TalosResult<pb::PluginSourceRemoveParams> {
    encode_plugin_source_remove_request(value)
}
pub fn decode_remove_plugin_source_params(
    value: Option<pb::PluginSourceRemoveParams>,
) -> TalosResult<d::PluginSourceRemoveRequest> {
    decode_plugin_source_remove_request(value.ok_or_else(invalid)?)
}
pub fn encode_remove_plugin_source_result(
    value: d::PluginSourceRemoveResponse,
) -> TalosResult<pb::PluginSourceRemoveResult> {
    encode_plugin_source_remove_response(value)
}
pub fn decode_remove_plugin_source_result(
    value: pb::PluginSourceRemoveResult,
) -> TalosResult<d::PluginSourceRemoveResponse> {
    decode_plugin_source_remove_response(value)
}
pub fn encode_search_plugins_params(
    value: d::PluginSearchRequest,
) -> TalosResult<pb::PluginSearchParams> {
    encode_plugin_search_request(value)
}
pub fn decode_search_plugins_params(
    value: Option<pb::PluginSearchParams>,
) -> TalosResult<d::PluginSearchRequest> {
    decode_plugin_search_request(value.ok_or_else(invalid)?)
}
pub fn encode_search_plugins_result(
    value: d::PluginSearchResponse,
) -> TalosResult<pb::PluginSearchResult> {
    encode_plugin_search_response(value)
}
pub fn decode_search_plugins_result(
    value: pb::PluginSearchResult,
) -> TalosResult<d::PluginSearchResponse> {
    decode_plugin_search_response(value)
}
pub fn encode_list_plugins_params(
    value: d::PluginListRequest,
) -> TalosResult<pb::PluginListParams> {
    encode_plugin_list_request(value)
}
pub fn decode_list_plugins_params(
    value: Option<pb::PluginListParams>,
) -> TalosResult<d::PluginListRequest> {
    decode_plugin_list_request(value.ok_or_else(invalid)?)
}
pub fn encode_list_plugins_result(
    value: d::PluginListResponse,
) -> TalosResult<pb::PluginListResult> {
    encode_plugin_list_response(value)
}
pub fn decode_list_plugins_result(
    value: pb::PluginListResult,
) -> TalosResult<d::PluginListResponse> {
    decode_plugin_list_response(value)
}
pub fn encode_read_plugin_params(value: d::PluginReadRequest) -> TalosResult<pb::PluginReadParams> {
    encode_plugin_read_request(value)
}
pub fn decode_read_plugin_params(
    value: Option<pb::PluginReadParams>,
) -> TalosResult<d::PluginReadRequest> {
    decode_plugin_read_request(value.ok_or_else(invalid)?)
}
pub fn encode_read_plugin_result(
    value: d::PluginReadResponse,
) -> TalosResult<pb::PluginReadResult> {
    encode_plugin_read_response(value)
}
pub fn decode_read_plugin_result(
    value: pb::PluginReadResult,
) -> TalosResult<d::PluginReadResponse> {
    decode_plugin_read_response(value)
}
pub fn encode_install_plugin_params(
    value: d::PluginInstallRequest,
) -> TalosResult<pb::PluginInstallParams> {
    encode_plugin_install_request(value)
}
pub fn decode_install_plugin_params(
    value: Option<pb::PluginInstallParams>,
) -> TalosResult<d::PluginInstallRequest> {
    decode_plugin_install_request(value.ok_or_else(invalid)?)
}
pub fn encode_install_plugin_result(
    value: d::PluginInstallResponse,
) -> TalosResult<pb::PluginInstallResult> {
    encode_plugin_install_response(value)
}
pub fn decode_install_plugin_result(
    value: pb::PluginInstallResult,
) -> TalosResult<d::PluginInstallResponse> {
    decode_plugin_install_response(value)
}
pub fn encode_update_plugin_params(
    value: d::PluginUpdateRequest,
) -> TalosResult<pb::PluginUpdateParams> {
    encode_plugin_update_request(value)
}
pub fn decode_update_plugin_params(
    value: Option<pb::PluginUpdateParams>,
) -> TalosResult<d::PluginUpdateRequest> {
    decode_plugin_update_request(value.ok_or_else(invalid)?)
}
pub fn encode_update_plugin_result(
    value: d::PluginUpdateResponse,
) -> TalosResult<pb::PluginUpdateResult> {
    encode_plugin_update_response(value)
}
pub fn decode_update_plugin_result(
    value: pb::PluginUpdateResult,
) -> TalosResult<d::PluginUpdateResponse> {
    decode_plugin_update_response(value)
}
pub fn encode_uninstall_plugin_params(
    value: d::PluginUninstallRequest,
) -> TalosResult<pb::PluginUninstallParams> {
    encode_plugin_uninstall_request(value)
}
pub fn decode_uninstall_plugin_params(
    value: Option<pb::PluginUninstallParams>,
) -> TalosResult<d::PluginUninstallRequest> {
    decode_plugin_uninstall_request(value.ok_or_else(invalid)?)
}
pub fn encode_uninstall_plugin_result(
    value: d::PluginUninstallResponse,
) -> TalosResult<pb::PluginUninstallResult> {
    encode_plugin_uninstall_response(value)
}
pub fn decode_uninstall_plugin_result(
    value: pb::PluginUninstallResult,
) -> TalosResult<d::PluginUninstallResponse> {
    decode_plugin_uninstall_response(value)
}
pub fn encode_enable_plugin_params(
    value: d::PluginEnableRequest,
) -> TalosResult<pb::PluginEnableParams> {
    encode_plugin_enable_request(value)
}
pub fn decode_enable_plugin_params(
    value: Option<pb::PluginEnableParams>,
) -> TalosResult<d::PluginEnableRequest> {
    decode_plugin_enable_request(value.ok_or_else(invalid)?)
}
pub fn encode_enable_plugin_result(
    value: d::PluginEnableResponse,
) -> TalosResult<pb::PluginEnableResult> {
    encode_plugin_enable_response(value)
}
pub fn decode_enable_plugin_result(
    value: pb::PluginEnableResult,
) -> TalosResult<d::PluginEnableResponse> {
    decode_plugin_enable_response(value)
}
pub fn encode_disable_plugin_params(
    value: d::PluginDisableRequest,
) -> TalosResult<pb::PluginDisableParams> {
    encode_plugin_disable_request(value)
}
pub fn decode_disable_plugin_params(
    value: Option<pb::PluginDisableParams>,
) -> TalosResult<d::PluginDisableRequest> {
    decode_plugin_disable_request(value.ok_or_else(invalid)?)
}
pub fn encode_disable_plugin_result(
    value: d::PluginDisableResponse,
) -> TalosResult<pb::PluginDisableResult> {
    encode_plugin_disable_response(value)
}
pub fn decode_disable_plugin_result(
    value: pb::PluginDisableResult,
) -> TalosResult<d::PluginDisableResponse> {
    decode_plugin_disable_response(value)
}
pub fn encode_list_mcp_servers_params(
    value: d::McpServerListRequest,
) -> TalosResult<pb::McpServerListParams> {
    encode_mcp_server_list_request(value)
}
pub fn decode_list_mcp_servers_params(
    value: Option<pb::McpServerListParams>,
) -> TalosResult<d::McpServerListRequest> {
    decode_mcp_server_list_request(value.ok_or_else(invalid)?)
}
pub fn encode_list_mcp_servers_result(
    value: d::McpServerListResponse,
) -> TalosResult<pb::McpServerListResult> {
    encode_mcp_server_list_response(value)
}
pub fn decode_list_mcp_servers_result(
    value: pb::McpServerListResult,
) -> TalosResult<d::McpServerListResponse> {
    decode_mcp_server_list_response(value)
}
pub fn encode_read_mcp_authentication_params(
    value: d::McpAuthenticationReadRequest,
) -> TalosResult<pb::McpAuthenticationReadParams> {
    encode_mcp_authentication_read_request(value)
}
pub fn decode_read_mcp_authentication_params(
    value: Option<pb::McpAuthenticationReadParams>,
) -> TalosResult<d::McpAuthenticationReadRequest> {
    decode_mcp_authentication_read_request(value.ok_or_else(invalid)?)
}
pub fn encode_read_mcp_authentication_result(
    value: d::McpAuthenticationResponse,
) -> TalosResult<pb::McpAuthenticationResult> {
    encode_mcp_authentication_response(value)
}
pub fn decode_read_mcp_authentication_result(
    value: pb::McpAuthenticationResult,
) -> TalosResult<d::McpAuthenticationResponse> {
    decode_mcp_authentication_response(value)
}
pub fn encode_update_mcp_credentials_params(
    value: d::McpCredentialsUpdateRequest,
) -> TalosResult<pb::McpCredentialsUpdateParams> {
    encode_mcp_credentials_update_request(value)
}
pub fn decode_update_mcp_credentials_params(
    value: Option<pb::McpCredentialsUpdateParams>,
) -> TalosResult<d::McpCredentialsUpdateRequest> {
    decode_mcp_credentials_update_request(value.ok_or_else(invalid)?)
}
pub fn encode_update_mcp_credentials_result(
    value: d::McpAuthenticationResponse,
) -> TalosResult<pb::McpAuthenticationResult> {
    encode_mcp_authentication_response(value)
}
pub fn decode_update_mcp_credentials_result(
    value: pb::McpAuthenticationResult,
) -> TalosResult<d::McpAuthenticationResponse> {
    decode_mcp_authentication_response(value)
}
pub fn encode_import_mcp_server_params(
    value: d::McpServerImportRequest,
) -> TalosResult<pb::McpServerImportParams> {
    encode_mcp_server_import_request(value)
}
pub fn decode_import_mcp_server_params(
    value: Option<pb::McpServerImportParams>,
) -> TalosResult<d::McpServerImportRequest> {
    decode_mcp_server_import_request(value.ok_or_else(invalid)?)
}
pub fn encode_import_mcp_server_result(
    value: d::PluginInstallResponse,
) -> TalosResult<pb::PluginInstallResult> {
    encode_plugin_install_response(value)
}
pub fn decode_import_mcp_server_result(
    value: pb::PluginInstallResult,
) -> TalosResult<d::PluginInstallResponse> {
    decode_plugin_install_response(value)
}
pub fn encode_start_mcp_authentication_params(
    value: d::McpAuthenticationStartRequest,
) -> TalosResult<pb::McpAuthenticationStartParams> {
    encode_mcp_authentication_start_request(value)
}
pub fn decode_start_mcp_authentication_params(
    value: Option<pb::McpAuthenticationStartParams>,
) -> TalosResult<d::McpAuthenticationStartRequest> {
    decode_mcp_authentication_start_request(value.ok_or_else(invalid)?)
}
pub fn encode_start_mcp_authentication_result(
    value: d::McpAuthenticationStartResponse,
) -> TalosResult<pb::McpAuthenticationStartResult> {
    encode_mcp_authentication_start_response(value)
}
pub fn decode_start_mcp_authentication_result(
    value: pb::McpAuthenticationStartResult,
) -> TalosResult<d::McpAuthenticationStartResponse> {
    decode_mcp_authentication_start_response(value)
}

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;

pub(crate) use mcp_authentication_status::{
    decode_mcp_authentication_response, encode_mcp_authentication_response,
};
pub(crate) use mcp_credentials::{decode_mcp_definition_ref, encode_mcp_definition_ref};
