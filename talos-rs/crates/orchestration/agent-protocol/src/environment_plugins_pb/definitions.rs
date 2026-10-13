use super::*;
pub(super) fn encode_plugin(value: d::Plugin) -> TalosResult<pb::Plugin> {
    Ok(pb::Plugin {
        id: value.id.0,
        kind: encode_plugin_kind(value.kind)?,
        metadata: Some(encode_plugin_metadata(value.metadata)?),
        documents: value
            .documents
            .into_iter()
            .map(encode_plugin_document_ref)
            .collect::<TalosResult<_>>()?,
        source_id: value.source_id.map(|value| value.0),
        path_in_source: value
            .path_in_source
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source_commit: value
            .source_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        status: Some(encode_plugin_status(value.status)?),
        definition: Some(encode_plugin_definition(value.definition)?),
        warnings: value
            .warnings
            .into_iter()
            .map(encode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn decode_plugin(value: pb::Plugin) -> TalosResult<d::Plugin> {
    Ok(d::Plugin {
        id: d::PluginId(value.id),
        kind: decode_plugin_kind(value.kind)?,
        metadata: decode_plugin_metadata(value.metadata.ok_or_else(invalid)?)?,
        documents: value
            .documents
            .into_iter()
            .map(decode_plugin_document_ref)
            .collect::<TalosResult<_>>()?,
        source_id: value.source_id.map(d::MarketplaceSourceId),
        path_in_source: value
            .path_in_source
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        source_commit: value
            .source_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        status: decode_plugin_status(value.status.ok_or_else(invalid)?)?,
        definition: decode_plugin_definition(value.definition.ok_or_else(invalid)?)?,
        warnings: value
            .warnings
            .into_iter()
            .map(decode_plugin_warning)
            .collect::<TalosResult<_>>()?,
    })
}
pub(super) fn encode_plugin_definition(
    value: d::PluginDefinition,
) -> TalosResult<pb::PluginDefinition> {
    Ok(pb::PluginDefinition {
        value: Some(match value {
            d::PluginDefinition::Skill(value) => {
                pb::plugin_definition::Value::Skill(encode_skill_plugin(value)?)
            }
            d::PluginDefinition::McpServer(value) => {
                pb::plugin_definition::Value::McpServer(encode_mcp_server_plugin(value)?)
            }
            d::PluginDefinition::Package(value) => {
                pb::plugin_definition::Value::Package(encode_package_plugin(value)?)
            }
            d::PluginDefinition::Feature(value) => {
                pb::plugin_definition::Value::Feature(encode_feature_plugin(value)?)
            }
        }),
    })
}
pub(super) fn decode_plugin_definition(
    value: pb::PluginDefinition,
) -> TalosResult<d::PluginDefinition> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::plugin_definition::Value::Skill(value) => {
            d::PluginDefinition::Skill(decode_skill_plugin(value)?)
        }
        pb::plugin_definition::Value::McpServer(value) => {
            d::PluginDefinition::McpServer(decode_mcp_server_plugin(value)?)
        }
        pb::plugin_definition::Value::Package(value) => {
            d::PluginDefinition::Package(decode_package_plugin(value)?)
        }
        pb::plugin_definition::Value::Feature(value) => {
            d::PluginDefinition::Feature(decode_feature_plugin(value)?)
        }
    })
}
pub(super) fn encode_plugin_document_media_type(
    value: d::PluginDocumentMediaType,
) -> TalosResult<i32> {
    Ok(match value {
        d::PluginDocumentMediaType::Markdown => 1,
        d::PluginDocumentMediaType::Json => 2,
        d::PluginDocumentMediaType::Text => 3,
    })
}
pub(super) fn decode_plugin_document_media_type(
    value: i32,
) -> TalosResult<d::PluginDocumentMediaType> {
    match value {
        1 => Ok(d::PluginDocumentMediaType::Markdown),
        2 => Ok(d::PluginDocumentMediaType::Json),
        3 => Ok(d::PluginDocumentMediaType::Text),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_document_ref(
    value: d::PluginDocumentRef,
) -> TalosResult<pb::PluginDocumentRef> {
    Ok(pb::PluginDocumentRef {
        role: encode_plugin_document_role(value.role)?,
        path: value.path,
        title: value.title,
        media_type: encode_plugin_document_media_type(value.media_type)?,
    })
}
pub(super) fn decode_plugin_document_ref(
    value: pb::PluginDocumentRef,
) -> TalosResult<d::PluginDocumentRef> {
    Ok(d::PluginDocumentRef {
        role: decode_plugin_document_role(value.role)?,
        path: value.path,
        title: value.title,
        media_type: decode_plugin_document_media_type(value.media_type)?,
    })
}
pub(super) fn encode_plugin_document_role(value: d::PluginDocumentRole) -> TalosResult<i32> {
    Ok(match value {
        d::PluginDocumentRole::Primary => 1,
        d::PluginDocumentRole::Readme => 2,
        d::PluginDocumentRole::Manifest => 3,
    })
}
pub(super) fn decode_plugin_document_role(value: i32) -> TalosResult<d::PluginDocumentRole> {
    match value {
        1 => Ok(d::PluginDocumentRole::Primary),
        2 => Ok(d::PluginDocumentRole::Readme),
        3 => Ok(d::PluginDocumentRole::Manifest),
        _ => Err(invalid()),
    }
}
