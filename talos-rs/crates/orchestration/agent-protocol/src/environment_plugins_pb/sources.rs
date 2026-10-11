use super::*;
pub(super) fn encode_marketplace_source(
    value: d::MarketplaceSource,
) -> TalosResult<pb::MarketplaceSource> {
    Ok(pb::MarketplaceSource {
        id: value.id.0,
        source_type: encode_marketplace_source_type(value.source_type)?,
        kind: encode_marketplace_source_kind(value.kind)?,
        display_name: value.display_name,
        location: Some(encode_marketplace_source_location(value.location)?),
        sparse_paths: value
            .sparse_paths
            .into_iter()
            .map(Ok)
            .collect::<TalosResult<_>>()?,
        last_checked_commit: value
            .last_checked_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        last_checked_at_ms: value
            .last_checked_at_ms
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        created_at_ms: value.created_at_ms,
        updated_at_ms: value.updated_at_ms,
    })
}
pub(super) fn decode_marketplace_source(
    value: pb::MarketplaceSource,
) -> TalosResult<d::MarketplaceSource> {
    Ok(d::MarketplaceSource {
        id: d::MarketplaceSourceId(value.id),
        source_type: decode_marketplace_source_type(value.source_type)?,
        kind: decode_marketplace_source_kind(value.kind)?,
        display_name: value.display_name,
        location: decode_marketplace_source_location(value.location.ok_or_else(invalid)?)?,
        sparse_paths: value
            .sparse_paths
            .into_iter()
            .map(Ok)
            .collect::<TalosResult<_>>()?,
        last_checked_commit: value
            .last_checked_commit
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        last_checked_at_ms: value
            .last_checked_at_ms
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        created_at_ms: value.created_at_ms,
        updated_at_ms: value.updated_at_ms,
    })
}
pub(super) fn encode_marketplace_source_input(
    value: d::MarketplaceSourceInput,
) -> TalosResult<pb::MarketplaceSourceInput> {
    Ok(pb::MarketplaceSourceInput {
        value: Some(match value {
            d::MarketplaceSourceInput::Git {
                url,
                reference,
                sparse_paths,
            } => pb::marketplace_source_input::Value::Git(pb::MarketplaceSourceInputGit {
                url,
                reference,
                sparse_paths: sparse_paths
                    .into_iter()
                    .map(Ok)
                    .collect::<TalosResult<_>>()?,
            }),
            d::MarketplaceSourceInput::LocalFolder { path, sparse_paths } => {
                pb::marketplace_source_input::Value::LocalFolder(
                    pb::MarketplaceSourceInputLocalFolder {
                        path: path.to_str().ok_or_else(invalid)?.to_owned(),
                        sparse_paths: sparse_paths
                            .into_iter()
                            .map(Ok)
                            .collect::<TalosResult<_>>()?,
                    },
                )
            }
        }),
    })
}
pub(super) fn decode_marketplace_source_input(
    value: pb::MarketplaceSourceInput,
) -> TalosResult<d::MarketplaceSourceInput> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::marketplace_source_input::Value::Git(value) => d::MarketplaceSourceInput::Git {
            url: value.url,
            reference: value.reference,
            sparse_paths: value
                .sparse_paths
                .into_iter()
                .map(Ok)
                .collect::<TalosResult<_>>()?,
        },
        pb::marketplace_source_input::Value::LocalFolder(value) => {
            d::MarketplaceSourceInput::LocalFolder {
                path: value.path.into(),
                sparse_paths: value
                    .sparse_paths
                    .into_iter()
                    .map(Ok)
                    .collect::<TalosResult<_>>()?,
            }
        }
    })
}
pub(super) fn encode_marketplace_source_kind(value: d::MarketplaceSourceKind) -> TalosResult<i32> {
    Ok(match value {
        d::MarketplaceSourceKind::Git => 1,
        d::MarketplaceSourceKind::LocalFolder => 2,
    })
}
pub(super) fn decode_marketplace_source_kind(value: i32) -> TalosResult<d::MarketplaceSourceKind> {
    match value {
        1 => Ok(d::MarketplaceSourceKind::Git),
        2 => Ok(d::MarketplaceSourceKind::LocalFolder),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_marketplace_source_location(
    value: d::MarketplaceSourceLocation,
) -> TalosResult<pb::MarketplaceSourceLocation> {
    Ok(pb::MarketplaceSourceLocation {
        value: Some(match value {
            d::MarketplaceSourceLocation::Git { url, reference } => {
                pb::marketplace_source_location::Value::Git(pb::MarketplaceSourceLocationGit {
                    url,
                    reference,
                })
            }
            d::MarketplaceSourceLocation::LocalFolder { path } => {
                pb::marketplace_source_location::Value::LocalFolder(
                    pb::MarketplaceSourceLocationLocalFolder {
                        path: path.to_str().ok_or_else(invalid)?.to_owned(),
                    },
                )
            }
        }),
    })
}
pub(super) fn decode_marketplace_source_location(
    value: pb::MarketplaceSourceLocation,
) -> TalosResult<d::MarketplaceSourceLocation> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::marketplace_source_location::Value::Git(value) => d::MarketplaceSourceLocation::Git {
            url: value.url,
            reference: value.reference,
        },
        pb::marketplace_source_location::Value::LocalFolder(value) => {
            d::MarketplaceSourceLocation::LocalFolder {
                path: value.path.into(),
            }
        }
    })
}
pub(super) fn encode_marketplace_source_type(value: d::MarketplaceSourceType) -> TalosResult<i32> {
    Ok(match value {
        d::MarketplaceSourceType::Official => 1,
        d::MarketplaceSourceType::User => 2,
    })
}
pub(super) fn decode_marketplace_source_type(value: i32) -> TalosResult<d::MarketplaceSourceType> {
    match value {
        1 => Ok(d::MarketplaceSourceType::Official),
        2 => Ok(d::MarketplaceSourceType::User),
        _ => Err(invalid()),
    }
}
