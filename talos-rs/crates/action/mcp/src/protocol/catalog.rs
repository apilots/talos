use super::{
    check_size, map_service_error, request_response, DEFAULT_TOOL_TIMEOUT, MAX_RECORDS,
    MAX_RESPONSE_BYTES,
};
use rmcp::{
    model::{ClientRequest, PaginatedRequestParams, ServerResult},
    service::ServiceError,
    RoleClient,
};
use serde_json::Value;
use std::future::Future;
use talos_error::{TalosError, TalosResult};
use talos_types::{McpResourceDescriptor, McpResourceTemplateDescriptor, McpToolDescriptor};

pub(crate) async fn list_tools(
    server_name: &str,
    service: &rmcp::Peer<RoleClient>,
) -> TalosResult<Vec<McpToolDescriptor>> {
    if service
        .peer_info()
        .is_none_or(|info| info.capabilities.tools.is_none())
    {
        return Ok(vec![]);
    }
    let mut tools = Vec::new();
    let mut cursor = None;
    let mut seen = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    let mut bytes = 0usize;
    for _ in 0..100 {
        let result = service
            .list_tools(
                cursor.map(|cursor| PaginatedRequestParams::default().with_cursor(Some(cursor))),
            )
            .await
            .map_err(map_service_error)?;
        bytes = bytes.saturating_add(check_size(&result)?);
        if bytes > MAX_RESPONSE_BYTES || tools.len() + result.tools.len() > MAX_RECORDS {
            return Err(catalog_error());
        }
        for tool in &result.tools {
            if tool.input_schema.get("type").and_then(Value::as_str) != Some("object")
                || !names.insert(tool.name.to_string())
            {
                return Err(TalosError::new(
                    talos_error::ErrorCode::ToolExecutionFailed,
                    "The MCP tool catalog contains an invalid schema or duplicate name.",
                ));
            }
        }
        tools.extend(
            result
                .tools
                .into_iter()
                .map(|tool| {
                    Ok(McpToolDescriptor {
                        server_name: server_name.to_string(),
                        name: tool.name.to_string(),
                        title: tool.title,
                        description: tool.description.map(std::borrow::Cow::into_owned),
                        input_schema: Value::Object((*tool.input_schema).clone()),
                        output_schema: tool
                            .output_schema
                            .map(|schema| Value::Object((*schema).clone())),
                        annotations: tool
                            .annotations
                            .map(serde_json::to_value)
                            .transpose()
                            .map_err(|error| {
                                TalosError::internal_with_source(
                                    "Failed to encode MCP tool annotations.",
                                    error,
                                )
                            })?,
                    })
                })
                .collect::<TalosResult<Vec<_>>>()?,
        );
        cursor = result.next_cursor;
        let Some(next) = &cursor else {
            return Ok(tools);
        };
        if !seen.insert(next.clone()) {
            return Err(catalog_error());
        }
    }
    Err(catalog_error())
}

fn catalog_error() -> TalosError {
    TalosError::new(
        talos_error::ErrorCode::ToolExecutionFailed,
        "The MCP catalog exceeds its pagination or size limit.",
    )
}

pub(crate) async fn list_resources(
    server_name: &str,
    service: &rmcp::Peer<RoleClient>,
    cursor: Option<String>,
    cancellation: impl Future<Output = ()> + Send,
) -> TalosResult<talos_types::McpResourceListResponse> {
    let request =
        ClientRequest::ListResourcesRequest(rmcp::model::ListResourcesRequest::with_param(
            PaginatedRequestParams::default().with_cursor(cursor),
        ));
    let ServerResult::ListResourcesResult(result) =
        request_response(service, request, cancellation, DEFAULT_TOOL_TIMEOUT).await?
    else {
        return Err(map_service_error(ServiceError::UnexpectedResponse));
    };
    if result.resources.len() > MAX_RECORDS {
        return Err(catalog_error());
    }
    Ok(talos_types::McpResourceListResponse {
        next_cursor: result.next_cursor,
        data: result
            .resources
            .into_iter()
            .map(|resource| McpResourceDescriptor {
                server_name: server_name.into(),
                uri: resource.uri,
                name: Some(resource.name),
                description: resource.description,
                mime_type: resource.mime_type,
            })
            .collect(),
    })
}

pub(crate) async fn list_resource_templates(
    server_name: &str,
    service: &rmcp::Peer<RoleClient>,
    cursor: Option<String>,
    cancellation: impl Future<Output = ()> + Send,
) -> TalosResult<talos_types::McpResourceTemplateListResponse> {
    let request = ClientRequest::ListResourceTemplatesRequest(
        rmcp::model::ListResourceTemplatesRequest::with_param(
            PaginatedRequestParams::default().with_cursor(cursor),
        ),
    );
    let ServerResult::ListResourceTemplatesResult(result) =
        request_response(service, request, cancellation, DEFAULT_TOOL_TIMEOUT).await?
    else {
        return Err(map_service_error(ServiceError::UnexpectedResponse));
    };
    if result.resource_templates.len() > MAX_RECORDS {
        return Err(catalog_error());
    }
    Ok(talos_types::McpResourceTemplateListResponse {
        next_cursor: result.next_cursor,
        data: result
            .resource_templates
            .into_iter()
            .map(|resource| McpResourceTemplateDescriptor {
                server_name: server_name.into(),
                uri_template: resource.uri_template,
                name: Some(resource.name),
                description: resource.description,
                mime_type: resource.mime_type,
            })
            .collect(),
    })
}
