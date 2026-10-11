use super::*;
pub(super) fn encode_content_digest(value: d::ContentDigest) -> TalosResult<pb::ContentDigest> {
    Ok(pb::ContentDigest {
        algorithm: encode_content_digest_algorithm(value.algorithm)?,
        value: value.value,
        file_count: value.file_count,
        total_size_bytes: value.total_size_bytes,
    })
}
pub(super) fn decode_content_digest(value: pb::ContentDigest) -> TalosResult<d::ContentDigest> {
    Ok(d::ContentDigest {
        algorithm: decode_content_digest_algorithm(value.algorithm)?,
        value: value.value,
        file_count: value.file_count,
        total_size_bytes: value.total_size_bytes,
    })
}
pub(super) fn encode_content_digest_algorithm(
    value: d::ContentDigestAlgorithm,
) -> TalosResult<i32> {
    Ok(match value {
        d::ContentDigestAlgorithm::Sha256 => 1,
    })
}
pub(super) fn decode_content_digest_algorithm(
    value: i32,
) -> TalosResult<d::ContentDigestAlgorithm> {
    match value {
        1 => Ok(d::ContentDigestAlgorithm::Sha256),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_install(value: d::PluginInstall) -> TalosResult<pb::PluginInstall> {
    Ok(pb::PluginInstall {
        id: value.id.0,
        plugin_id: value.plugin_id.0,
        kind: encode_plugin_kind(value.kind)?,
        name: value.name,
        installed_path: value
            .installed_path
            .to_str()
            .ok_or_else(invalid)?
            .to_owned(),
        enabled: value.enabled,
        source: Some(encode_plugin_install_source(value.source)?),
        content_digest: Some(encode_content_digest(value.content_digest)?),
        installed_at_ms: value.installed_at_ms,
        updated_at_ms: value.updated_at_ms,
    })
}
pub(super) fn decode_plugin_install(value: pb::PluginInstall) -> TalosResult<d::PluginInstall> {
    Ok(d::PluginInstall {
        id: d::PluginInstallId(value.id),
        plugin_id: d::PluginId(value.plugin_id),
        kind: decode_plugin_kind(value.kind)?,
        name: value.name,
        installed_path: value.installed_path.into(),
        enabled: value.enabled,
        source: decode_plugin_install_source(value.source.ok_or_else(invalid)?)?,
        content_digest: decode_content_digest(value.content_digest.ok_or_else(invalid)?)?,
        installed_at_ms: value.installed_at_ms,
        updated_at_ms: value.updated_at_ms,
    })
}
pub(super) fn encode_plugin_install_source(
    value: d::PluginInstallSource,
) -> TalosResult<pb::PluginInstallSource> {
    Ok(pb::PluginInstallSource {
        value: Some(match value {
            d::PluginInstallSource::Marketplace {
                source_id,
                url,
                reference,
                commit,
                path_in_source,
            } => {
                pb::plugin_install_source::Value::Marketplace(pb::PluginInstallSourceMarketplace {
                    source_id: source_id.0,
                    url,
                    reference,
                    commit,
                    path_in_source,
                })
            }
            d::PluginInstallSource::LocalPath {
                source_id,
                path,
                path_in_source,
            } => pb::plugin_install_source::Value::LocalPath(pb::PluginInstallSourceLocalPath {
                source_id: source_id.0,
                path: path.to_str().ok_or_else(invalid)?.to_owned(),
                path_in_source,
            }),
            d::PluginInstallSource::Manual => {
                pb::plugin_install_source::Value::Manual(pb::Empty {})
            }
        }),
    })
}
pub(super) fn decode_plugin_install_source(
    value: pb::PluginInstallSource,
) -> TalosResult<d::PluginInstallSource> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::plugin_install_source::Value::Marketplace(value) => {
            d::PluginInstallSource::Marketplace {
                source_id: d::MarketplaceSourceId(value.source_id),
                url: value.url,
                reference: value.reference,
                commit: value.commit,
                path_in_source: value.path_in_source,
            }
        }
        pb::plugin_install_source::Value::LocalPath(value) => d::PluginInstallSource::LocalPath {
            source_id: d::MarketplaceSourceId(value.source_id),
            path: value.path.into(),
            path_in_source: value.path_in_source,
        },
        pb::plugin_install_source::Value::Manual(_) => d::PluginInstallSource::Manual,
    })
}
