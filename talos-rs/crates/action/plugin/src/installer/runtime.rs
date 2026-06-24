use crate::documents::{mcp_manifest_document_ref, skill_document_ref};
use crate::scanner::mcp_manifest::server_from_installed_mcp_manifest;
use talos_error::TalosResult;
use talos_types::{
    Plugin, PluginDefinition, PluginInstall, PluginInstallSource, PluginKind, PluginMetadata,
    PluginStatus,
};

pub(crate) fn installed_skill_root(install: &PluginInstall) -> Option<talos_skills::SkillRoot> {
    (install.kind == PluginKind::Skill && install.enabled).then(|| {
        talos_skills::SkillRoot::plugin(
            install.installed_path.clone(),
            install.plugin_id.0.clone(),
            install.id.0.clone(),
        )
    })
}

pub(crate) fn installed_mcp_servers(
    install: &PluginInstall,
) -> TalosResult<Vec<talos_types::McpServerRuntimeDescriptor>> {
    if install.kind != PluginKind::McpServer || !install.enabled {
        return Ok(Vec::new());
    }
    let server = server_from_installed_mcp_manifest(&install.installed_path)?;
    Ok(vec![talos_types::McpServerRuntimeDescriptor {
        server,
        source: talos_types::McpServerSource::Plugin {
            plugin_id: install.plugin_id.clone(),
            install_id: install.id.clone(),
            installed_path: install.installed_path.clone(),
        },
    }])
}

pub(crate) fn installed_plugin(
    mut plugin: Plugin,
    install: &PluginInstall,
    enabled: bool,
) -> Plugin {
    plugin.status = PluginStatus::Installed {
        install_id: install.id.clone(),
        installed_path: install.installed_path.clone(),
        installed_commit: source_commit(install),
        enabled,
    };
    plugin
}

pub(crate) fn missing_plugin_from_install(install: &PluginInstall, enabled: bool) -> Plugin {
    let definition = match install.kind {
        PluginKind::Skill => PluginDefinition::Skill(talos_types::SkillPlugin {
            skill_md_path: "SKILL.md".to_string(),
        }),
        PluginKind::McpServer => PluginDefinition::McpServer(talos_types::McpServerPlugin {
            server: talos_types::McpServerDefinition {
                name: install.name.clone(),
                display_name: None,
                description: Some(install.name.clone()),
                transport: talos_types::McpTransportDefinition::Stdio {
                    command: String::new(),
                    args: Vec::new(),
                    env: Default::default(),
                    cwd: None,
                },
            },
            manifest_path: "talos-mcp.json".to_string(),
        }),
        PluginKind::Package => PluginDefinition::Package(talos_types::PackagePlugin {
            name: install.name.clone(),
            package_kind: talos_types::PackageKind::Binary,
            manifest_path: String::new(),
        }),
        PluginKind::Feature => PluginDefinition::Feature(talos_types::FeaturePlugin {
            name: install.name.clone(),
            feature_kind: talos_types::FeatureKind::Integration,
            manifest_path: String::new(),
        }),
    };
    Plugin {
        id: install.plugin_id.clone(),
        kind: install.kind,
        metadata: PluginMetadata {
            name: install.name.clone(),
            display_name: None,
            description: install.name.clone(),
            short_description: None,
            icon: None,
            license: None,
        },
        documents: missing_plugin_documents(install.kind),
        source_id: source_id(install),
        path_in_source: path_in_source(install),
        source_commit: source_commit(install),
        status: PluginStatus::SourceMissing {
            install_id: install.id.clone(),
            installed_path: install.installed_path.clone(),
            installed_commit: source_commit(install),
            enabled,
        },
        definition,
        warnings: Vec::new(),
    }
}

pub(crate) fn source_commit(install: &PluginInstall) -> Option<String> {
    match &install.source {
        PluginInstallSource::Marketplace { commit, .. } => Some(commit.clone()),
        PluginInstallSource::LocalPath { .. } | PluginInstallSource::Manual => None,
    }
}

fn source_id(install: &PluginInstall) -> Option<talos_types::MarketplaceSourceId> {
    match &install.source {
        PluginInstallSource::Marketplace { source_id, .. } => Some(source_id.clone()),
        PluginInstallSource::LocalPath { source_id, .. } => Some(source_id.clone()),
        PluginInstallSource::Manual => None,
    }
}

fn path_in_source(install: &PluginInstall) -> Option<String> {
    match &install.source {
        PluginInstallSource::Marketplace { path_in_source, .. } => Some(path_in_source.clone()),
        PluginInstallSource::LocalPath { path_in_source, .. } => Some(path_in_source.clone()),
        PluginInstallSource::Manual => None,
    }
}

fn missing_plugin_documents(kind: PluginKind) -> Vec<talos_types::PluginDocumentRef> {
    match kind {
        PluginKind::Skill => vec![skill_document_ref("SKILL.md".to_string())],
        PluginKind::McpServer => vec![mcp_manifest_document_ref("talos-mcp.json".to_string())],
        PluginKind::Package | PluginKind::Feature => Vec::new(),
    }
}
