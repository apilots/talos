use super::*;
pub(super) fn encode_plugin_conflict_reason(value: d::PluginConflictReason) -> TalosResult<i32> {
    Ok(match value {
        d::PluginConflictReason::InvalidManifest => 1,
        d::PluginConflictReason::SameNameDifferentSource => 2,
        d::PluginConflictReason::TargetPathExists => 3,
        d::PluginConflictReason::TargetPathMissing => 4,
        d::PluginConflictReason::LocalChangesDetected => 5,
        d::PluginConflictReason::UnsupportedKind => 6,
    })
}
pub(super) fn decode_plugin_conflict_reason(value: i32) -> TalosResult<d::PluginConflictReason> {
    match value {
        1 => Ok(d::PluginConflictReason::InvalidManifest),
        2 => Ok(d::PluginConflictReason::SameNameDifferentSource),
        3 => Ok(d::PluginConflictReason::TargetPathExists),
        4 => Ok(d::PluginConflictReason::TargetPathMissing),
        5 => Ok(d::PluginConflictReason::LocalChangesDetected),
        6 => Ok(d::PluginConflictReason::UnsupportedKind),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_status(value: d::PluginStatus) -> TalosResult<pb::PluginStatus> {
    Ok(pb::PluginStatus {
        value: Some(match value {
            d::PluginStatus::NotInstalled => pb::plugin_status::Value::NotInstalled(pb::Empty {}),
            d::PluginStatus::Installed {
                install_id,
                installed_path,
                installed_commit,
                enabled,
            } => pb::plugin_status::Value::Installed(pb::PluginStatusInstalled {
                install_id: install_id.0,
                installed_path: installed_path.to_str().ok_or_else(invalid)?.to_owned(),
                installed_commit: installed_commit
                    .map(|value| -> TalosResult<_> { Ok(value) })
                    .transpose()?,
                enabled,
            }),
            d::PluginStatus::UpdateAvailable {
                install_id,
                installed_path,
                installed_commit,
                latest_commit,
                enabled,
            } => pb::plugin_status::Value::UpdateAvailable(pb::PluginStatusUpdateAvailable {
                install_id: install_id.0,
                installed_path: installed_path.to_str().ok_or_else(invalid)?.to_owned(),
                installed_commit: installed_commit
                    .map(|value| -> TalosResult<_> { Ok(value) })
                    .transpose()?,
                latest_commit,
                enabled,
            }),
            d::PluginStatus::SourceMissing {
                install_id,
                installed_path,
                installed_commit,
                enabled,
            } => pb::plugin_status::Value::SourceMissing(pb::PluginStatusSourceMissing {
                install_id: install_id.0,
                installed_path: installed_path.to_str().ok_or_else(invalid)?.to_owned(),
                installed_commit: installed_commit
                    .map(|value| -> TalosResult<_> { Ok(value) })
                    .transpose()?,
                enabled,
            }),
            d::PluginStatus::Conflict { reason } => {
                pb::plugin_status::Value::Conflict(pb::PluginStatusConflict {
                    reason: encode_plugin_conflict_reason(reason)?,
                })
            }
        }),
    })
}
pub(super) fn decode_plugin_status(value: pb::PluginStatus) -> TalosResult<d::PluginStatus> {
    Ok(match value.value.ok_or_else(invalid)? {
        pb::plugin_status::Value::NotInstalled(_) => d::PluginStatus::NotInstalled,
        pb::plugin_status::Value::Installed(value) => d::PluginStatus::Installed {
            install_id: d::PluginInstallId(value.install_id),
            installed_path: value.installed_path.into(),
            installed_commit: value
                .installed_commit
                .map(|value| -> TalosResult<_> { Ok(value) })
                .transpose()?,
            enabled: value.enabled,
        },
        pb::plugin_status::Value::UpdateAvailable(value) => d::PluginStatus::UpdateAvailable {
            install_id: d::PluginInstallId(value.install_id),
            installed_path: value.installed_path.into(),
            installed_commit: value
                .installed_commit
                .map(|value| -> TalosResult<_> { Ok(value) })
                .transpose()?,
            latest_commit: value.latest_commit,
            enabled: value.enabled,
        },
        pb::plugin_status::Value::SourceMissing(value) => d::PluginStatus::SourceMissing {
            install_id: d::PluginInstallId(value.install_id),
            installed_path: value.installed_path.into(),
            installed_commit: value
                .installed_commit
                .map(|value| -> TalosResult<_> { Ok(value) })
                .transpose()?,
            enabled: value.enabled,
        },
        pb::plugin_status::Value::Conflict(value) => d::PluginStatus::Conflict {
            reason: decode_plugin_conflict_reason(value.reason)?,
        },
    })
}
pub(super) fn encode_plugin_warning(value: d::PluginWarning) -> TalosResult<pb::PluginWarning> {
    Ok(pb::PluginWarning {
        code: encode_plugin_warning_code(value.code)?,
        message: value.message,
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { encode_plugin_kind(value) })
            .transpose()?,
        source_id: value.source_id.map(|value| value.0),
        path: value
            .path
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn decode_plugin_warning(value: pb::PluginWarning) -> TalosResult<d::PluginWarning> {
    Ok(d::PluginWarning {
        code: decode_plugin_warning_code(value.code)?,
        message: value.message,
        kind: value
            .kind
            .map(|value| -> TalosResult<_> { decode_plugin_kind(value) })
            .transpose()?,
        source_id: value.source_id.map(d::MarketplaceSourceId),
        path: value
            .path
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn encode_plugin_warning_code(value: d::PluginWarningCode) -> TalosResult<i32> {
    Ok(match value {
        d::PluginWarningCode::InvalidManifest => 1,
        d::PluginWarningCode::MissingRequiredField => 2,
        d::PluginWarningCode::UnsupportedPluginKind => 3,
        d::PluginWarningCode::NestedPlugin => 4,
        d::PluginWarningCode::PathEscapedSource => 5,
        d::PluginWarningCode::InvalidSourceLayout => 6,
        d::PluginWarningCode::OrphanInstallDirectory => 7,
        d::PluginWarningCode::SourceRefreshFailed => 8,
    })
}
pub(super) fn decode_plugin_warning_code(value: i32) -> TalosResult<d::PluginWarningCode> {
    match value {
        1 => Ok(d::PluginWarningCode::InvalidManifest),
        2 => Ok(d::PluginWarningCode::MissingRequiredField),
        3 => Ok(d::PluginWarningCode::UnsupportedPluginKind),
        4 => Ok(d::PluginWarningCode::NestedPlugin),
        5 => Ok(d::PluginWarningCode::PathEscapedSource),
        6 => Ok(d::PluginWarningCode::InvalidSourceLayout),
        7 => Ok(d::PluginWarningCode::OrphanInstallDirectory),
        8 => Ok(d::PluginWarningCode::SourceRefreshFailed),
        _ => Err(invalid()),
    }
}
