use super::*;
pub(super) fn encode_plugin_source_add_request(
    value: d::PluginSourceAddRequest,
) -> TalosResult<pb::PluginSourceAddParams> {
    Ok(pb::PluginSourceAddParams {
        source: Some(encode_marketplace_source_input(value.source)?),
        refresh: value.refresh,
    })
}
pub(super) fn decode_plugin_source_add_request(
    value: pb::PluginSourceAddParams,
) -> TalosResult<d::PluginSourceAddRequest> {
    Ok(d::PluginSourceAddRequest {
        node_id: None,
        source: decode_marketplace_source_input(value.source.ok_or_else(invalid)?)?,
        refresh: value.refresh,
    })
}
pub(super) fn encode_plugin_source_add_response(
    value: d::PluginSourceAddResponse,
) -> TalosResult<pb::PluginSourceAddResult> {
    Ok(pb::PluginSourceAddResult {
        source: Some(encode_marketplace_source(value.source)?),
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
pub(super) fn decode_plugin_source_add_response(
    value: pb::PluginSourceAddResult,
) -> TalosResult<d::PluginSourceAddResponse> {
    Ok(d::PluginSourceAddResponse {
        source: decode_marketplace_source(value.source.ok_or_else(invalid)?)?,
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
pub(super) fn encode_plugin_source_list_request(
    _value: d::PluginSourceListRequest,
) -> TalosResult<pb::PluginSourceListParams> {
    Ok(pb::PluginSourceListParams {})
}
pub(super) fn decode_plugin_source_list_request(
    _value: pb::PluginSourceListParams,
) -> TalosResult<d::PluginSourceListRequest> {
    Ok(d::PluginSourceListRequest { node_id: None })
}
pub(super) fn encode_plugin_source_list_response(
    value: d::PluginSourceListResponse,
) -> TalosResult<pb::PluginSourceListResult> {
    Ok(pb::PluginSourceListResult {
        sources: value
            .sources
            .into_iter()
            .map(encode_marketplace_source)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_plugin_source_list_response(
    value: pb::PluginSourceListResult,
) -> TalosResult<d::PluginSourceListResponse> {
    Ok(d::PluginSourceListResponse {
        sources: value
            .sources
            .into_iter()
            .map(decode_marketplace_source)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn encode_plugin_source_refresh_request(
    value: d::PluginSourceRefreshRequest,
) -> TalosResult<pb::PluginSourceRefreshParams> {
    Ok(pb::PluginSourceRefreshParams {
        source_id: value.source_id.0,
    })
}
pub(super) fn decode_plugin_source_refresh_request(
    value: pb::PluginSourceRefreshParams,
) -> TalosResult<d::PluginSourceRefreshRequest> {
    Ok(d::PluginSourceRefreshRequest {
        node_id: None,
        source_id: d::MarketplaceSourceId(value.source_id),
    })
}
pub(super) fn encode_plugin_source_refresh_response(
    value: d::PluginSourceRefreshResponse,
) -> TalosResult<pb::PluginSourceRefreshResult> {
    Ok(pb::PluginSourceRefreshResult {
        source: Some(encode_marketplace_source(value.source)?),
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
pub(super) fn decode_plugin_source_refresh_response(
    value: pb::PluginSourceRefreshResult,
) -> TalosResult<d::PluginSourceRefreshResponse> {
    Ok(d::PluginSourceRefreshResponse {
        source: decode_marketplace_source(value.source.ok_or_else(invalid)?)?,
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
pub(super) fn encode_plugin_source_remove_request(
    value: d::PluginSourceRemoveRequest,
) -> TalosResult<pb::PluginSourceRemoveParams> {
    Ok(pb::PluginSourceRemoveParams {
        source_id: value.source_id.0,
    })
}
pub(super) fn decode_plugin_source_remove_request(
    value: pb::PluginSourceRemoveParams,
) -> TalosResult<d::PluginSourceRemoveRequest> {
    Ok(d::PluginSourceRemoveRequest {
        node_id: None,
        source_id: d::MarketplaceSourceId(value.source_id),
    })
}
pub(super) fn encode_plugin_source_remove_response(
    value: d::PluginSourceRemoveResponse,
) -> TalosResult<pb::PluginSourceRemoveResult> {
    Ok(pb::PluginSourceRemoveResult {
        source_id: value.source_id.0,
        removed: value.removed,
    })
}
pub(super) fn decode_plugin_source_remove_response(
    value: pb::PluginSourceRemoveResult,
) -> TalosResult<d::PluginSourceRemoveResponse> {
    Ok(d::PluginSourceRemoveResponse {
        source_id: d::MarketplaceSourceId(value.source_id),
        removed: value.removed,
    })
}
