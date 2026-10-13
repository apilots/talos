use super::*;
pub(super) fn encode_plugin_document(value: d::PluginDocument) -> TalosResult<pb::PluginDocument> {
    Ok(pb::PluginDocument {
        role: encode_plugin_document_role(value.role)?,
        path: value.path,
        title: value.title,
        media_type: encode_plugin_document_media_type(value.media_type)?,
        contents: value.contents,
    })
}
pub(super) fn decode_plugin_document(value: pb::PluginDocument) -> TalosResult<d::PluginDocument> {
    Ok(d::PluginDocument {
        role: decode_plugin_document_role(value.role)?,
        path: value.path,
        title: value.title,
        media_type: decode_plugin_document_media_type(value.media_type)?,
        contents: value.contents,
    })
}
pub(super) fn encode_plugin_list_request(
    value: d::PluginListRequest,
) -> TalosResult<pb::PluginListParams> {
    Ok(pb::PluginListParams {
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { encode_plugin_kind(value) })
            .transpose()?,
        status: value
            .status
            .map(|value| -> TalosResult<_> { encode_plugin_list_status_filter(value) })
            .transpose()?,
        source_id: value.source_id.map(|value| value.0),
        include_disabled: value.include_disabled,
    })
}
pub(super) fn decode_plugin_list_request(
    value: pb::PluginListParams,
) -> TalosResult<d::PluginListRequest> {
    Ok(d::PluginListRequest {
        node_id: None,
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { decode_plugin_kind(value) })
            .transpose()?,
        status: value
            .status
            .map(|value| -> TalosResult<_> { decode_plugin_list_status_filter(value) })
            .transpose()?,
        source_id: value.source_id.map(d::MarketplaceSourceId),
        include_disabled: value.include_disabled,
    })
}
pub(super) fn encode_plugin_list_response(
    value: d::PluginListResponse,
) -> TalosResult<pb::PluginListResult> {
    Ok(pb::PluginListResult {
        plugins: value
            .plugins
            .into_iter()
            .map(encode_plugin)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(encode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_plugin_list_response(
    value: pb::PluginListResult,
) -> TalosResult<d::PluginListResponse> {
    Ok(d::PluginListResponse {
        plugins: value
            .plugins
            .into_iter()
            .map(decode_plugin)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(decode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn encode_plugin_list_status_filter(
    value: d::PluginListStatusFilter,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginListStatusFilter::Installed => 1,
        d::PluginListStatusFilter::Disabled => 2,
        d::PluginListStatusFilter::UpdateAvailable => 3,
        d::PluginListStatusFilter::SourceMissing => 4,
        d::PluginListStatusFilter::Conflict => 5,
    })
}
pub(super) fn decode_plugin_list_status_filter(
    value: i32,
) -> TalosResult<d::PluginListStatusFilter> {
    match value {
        1 => Ok(d::PluginListStatusFilter::Installed),
        2 => Ok(d::PluginListStatusFilter::Disabled),
        3 => Ok(d::PluginListStatusFilter::UpdateAvailable),
        4 => Ok(d::PluginListStatusFilter::SourceMissing),
        5 => Ok(d::PluginListStatusFilter::Conflict),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_read_request(
    value: d::PluginReadRequest,
) -> TalosResult<pb::PluginReadParams> {
    Ok(pb::PluginReadParams {
        plugin_id: value.plugin_id.0,
    })
}
pub(super) fn decode_plugin_read_request(
    value: pb::PluginReadParams,
) -> TalosResult<d::PluginReadRequest> {
    Ok(d::PluginReadRequest {
        node_id: None,
        plugin_id: d::PluginId(value.plugin_id),
    })
}
pub(super) fn encode_plugin_read_response(
    value: d::PluginReadResponse,
) -> TalosResult<pb::PluginReadResult> {
    Ok(pb::PluginReadResult {
        plugin: Some(encode_plugin(value.plugin)?),
        documents: value
            .documents
            .into_iter()
            .map(encode_plugin_document)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_plugin_read_response(
    value: pb::PluginReadResult,
) -> TalosResult<d::PluginReadResponse> {
    Ok(d::PluginReadResponse {
        plugin: decode_plugin(value.plugin.ok_or_else(invalid)?)?,
        documents: value
            .documents
            .into_iter()
            .map(decode_plugin_document)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn encode_plugin_search_refresh_policy(
    value: d::PluginSearchRefreshPolicy,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginSearchRefreshPolicy::CacheOnly => 1,
        d::PluginSearchRefreshPolicy::RefreshStale => 2,
        d::PluginSearchRefreshPolicy::ForceRefresh => 3,
    })
}
pub(super) fn decode_plugin_search_refresh_policy(
    value: i32,
) -> TalosResult<d::PluginSearchRefreshPolicy> {
    match value {
        1 => Ok(d::PluginSearchRefreshPolicy::CacheOnly),
        2 => Ok(d::PluginSearchRefreshPolicy::RefreshStale),
        3 => Ok(d::PluginSearchRefreshPolicy::ForceRefresh),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_search_request(
    value: d::PluginSearchRequest,
) -> TalosResult<pb::PluginSearchParams> {
    Ok(pb::PluginSearchParams {
        query: value
            .query
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source_id: value.source_id.map(|value| value.0),
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { encode_plugin_kind(value) })
            .transpose()?,
        status: value
            .status
            .map(|value| -> TalosResult<_> { encode_plugin_search_status_filter(value) })
            .transpose()?,
        refresh: encode_plugin_search_refresh_policy(value.refresh)?,
        limit: value
            .limit
            .map(|value| -> TalosResult<_> { value.try_into().map_err(|_| invalid()) })
            .transpose()?,
    })
}
pub(super) fn decode_plugin_search_request(
    value: pb::PluginSearchParams,
) -> TalosResult<d::PluginSearchRequest> {
    Ok(d::PluginSearchRequest {
        node_id: None,
        query: value
            .query
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source_id: value.source_id.map(d::MarketplaceSourceId),
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { decode_plugin_kind(value) })
            .transpose()?,
        status: value
            .status
            .map(|value| -> TalosResult<_> { decode_plugin_search_status_filter(value) })
            .transpose()?,
        refresh: decode_plugin_search_refresh_policy(value.refresh)?,
        limit: value
            .limit
            .map(|value| -> TalosResult<_> { value.try_into().map_err(|_| invalid()) })
            .transpose()?,
    })
}
pub(super) fn encode_plugin_search_response(
    value: d::PluginSearchResponse,
) -> TalosResult<pb::PluginSearchResult> {
    Ok(pb::PluginSearchResult {
        plugins: value
            .plugins
            .into_iter()
            .map(encode_plugin)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(encode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_plugin_search_response(
    value: pb::PluginSearchResult,
) -> TalosResult<d::PluginSearchResponse> {
    Ok(d::PluginSearchResponse {
        plugins: value
            .plugins
            .into_iter()
            .map(decode_plugin)
            .collect::<TalosResult<_>>()?,
        warnings: value
            .warnings
            .into_iter()
            .map(decode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn encode_plugin_search_status_filter(
    value: d::PluginSearchStatusFilter,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginSearchStatusFilter::Any => 1,
        d::PluginSearchStatusFilter::NotInstalled => 2,
        d::PluginSearchStatusFilter::Installed => 3,
        d::PluginSearchStatusFilter::UpdateAvailable => 4,
        d::PluginSearchStatusFilter::Conflict => 5,
    })
}
pub(super) fn decode_plugin_search_status_filter(
    value: i32,
) -> TalosResult<d::PluginSearchStatusFilter> {
    match value {
        1 => Ok(d::PluginSearchStatusFilter::Any),
        2 => Ok(d::PluginSearchStatusFilter::NotInstalled),
        3 => Ok(d::PluginSearchStatusFilter::Installed),
        4 => Ok(d::PluginSearchStatusFilter::UpdateAvailable),
        5 => Ok(d::PluginSearchStatusFilter::Conflict),
        _ => Err(invalid()),
    }
}
