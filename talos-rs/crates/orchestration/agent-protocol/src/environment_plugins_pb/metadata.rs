use super::*;
pub(super) fn encode_feature_kind(value: d::FeatureKind) -> TalosResult<i32> {
    Ok(match value {
        d::FeatureKind::Ui => 1,
        d::FeatureKind::Runtime => 2,
        d::FeatureKind::Integration => 3,
    })
}
pub(super) fn decode_feature_kind(value: i32) -> TalosResult<d::FeatureKind> {
    match value {
        1 => Ok(d::FeatureKind::Ui),
        2 => Ok(d::FeatureKind::Runtime),
        3 => Ok(d::FeatureKind::Integration),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_feature_plugin(value: d::FeaturePlugin) -> TalosResult<pb::FeaturePlugin> {
    Ok(pb::FeaturePlugin {
        name: value.name,
        feature_kind: encode_feature_kind(value.feature_kind)?,
        manifest_path: value.manifest_path,
    })
}
pub(super) fn decode_feature_plugin(value: pb::FeaturePlugin) -> TalosResult<d::FeaturePlugin> {
    Ok(d::FeaturePlugin {
        name: value.name,
        feature_kind: decode_feature_kind(value.feature_kind)?,
        manifest_path: value.manifest_path,
    })
}
pub(super) fn encode_package_kind(value: d::PackageKind) -> TalosResult<i32> {
    Ok(match value {
        d::PackageKind::Npm => 1,
        d::PackageKind::Python => 2,
        d::PackageKind::Binary => 3,
    })
}
pub(super) fn decode_package_kind(value: i32) -> TalosResult<d::PackageKind> {
    match value {
        1 => Ok(d::PackageKind::Npm),
        2 => Ok(d::PackageKind::Python),
        3 => Ok(d::PackageKind::Binary),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_package_plugin(value: d::PackagePlugin) -> TalosResult<pb::PackagePlugin> {
    Ok(pb::PackagePlugin {
        name: value.name,
        package_kind: encode_package_kind(value.package_kind)?,
        manifest_path: value.manifest_path,
    })
}
pub(super) fn decode_package_plugin(value: pb::PackagePlugin) -> TalosResult<d::PackagePlugin> {
    Ok(d::PackagePlugin {
        name: value.name,
        package_kind: decode_package_kind(value.package_kind)?,
        manifest_path: value.manifest_path,
    })
}
pub(super) fn encode_plugin_kind(value: d::PluginKind) -> TalosResult<i32> {
    Ok(match value {
        d::PluginKind::Skill => 1,
        d::PluginKind::McpServer => 2,
        d::PluginKind::Package => 3,
        d::PluginKind::Feature => 4,
    })
}
pub(super) fn decode_plugin_kind(value: i32) -> TalosResult<d::PluginKind> {
    match value {
        1 => Ok(d::PluginKind::Skill),
        2 => Ok(d::PluginKind::McpServer),
        3 => Ok(d::PluginKind::Package),
        4 => Ok(d::PluginKind::Feature),
        _ => Err(invalid()),
    }
}
pub(super) fn encode_plugin_metadata(value: d::PluginMetadata) -> TalosResult<pb::PluginMetadata> {
    Ok(pb::PluginMetadata {
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        description: value.description,
        short_description: value
            .short_description
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        icon: value
            .icon
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        license: value
            .license
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn decode_plugin_metadata(value: pb::PluginMetadata) -> TalosResult<d::PluginMetadata> {
    Ok(d::PluginMetadata {
        name: value.name,
        display_name: value
            .display_name
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        description: value.description,
        short_description: value
            .short_description
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        icon: value
            .icon
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
        license: value
            .license
            .map(|value| -> TalosResult<_> { Ok(value) })
            .transpose()?,
    })
}
pub(super) fn encode_skill_plugin(value: d::SkillPlugin) -> TalosResult<pb::SkillPlugin> {
    Ok(pb::SkillPlugin {
        skill_md_path: value.skill_md_path,
    })
}
pub(super) fn decode_skill_plugin(value: pb::SkillPlugin) -> TalosResult<d::SkillPlugin> {
    Ok(d::SkillPlugin {
        skill_md_path: value.skill_md_path,
    })
}
