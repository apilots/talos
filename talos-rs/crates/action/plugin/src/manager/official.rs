use super::PluginManager;
use crate::{installer, scanner, source};
use serde::Deserialize;
use std::path::Path;
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_types::{
    McpServerDefinition, McpTransportDefinition, PluginDefinition, PluginInstall,
    PluginInstallConflictPolicy, PluginInstallSource, PluginKind, BROWSER_MCP_SERVER_NAME,
    OFFICIAL_BROWSER_PLUGIN_ID,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfficialBrowserProvision {
    Ready,
    NameConflict,
    Unavailable,
}

/// Build metadata for a product-supplied snapshot, not plugin installation state.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfficialSnapshot {
    repository: String,
    commit: String,
}

pub(crate) fn is_official_browser(install: &PluginInstall) -> bool {
    let expected = talos_config::official_mcp_source();
    install.plugin_id.0 == OFFICIAL_BROWSER_PLUGIN_ID
        && matches!(&install.source, PluginInstallSource::Marketplace {
            source_id, url, reference, commit, path_in_source,
        } if *source_id == source::source_id_for(&expected.url, &expected.reference, &expected.sparse_paths)
            && *url == expected.url && *reference == expected.reference
            && path_in_source == "mcp/browser" && valid_commit(commit))
}

fn valid_commit(commit: &str) -> bool {
    commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(crate) fn validate_browser_definition(server: &McpServerDefinition) -> TalosResult<()> {
    if server.name != BROWSER_MCP_SERVER_NAME
        || server.credentials.is_some()
        || !matches!(&server.transport, McpTransportDefinition::Stdio {command,args,env,..}
            if command == "talos" && args == &["--browser-mcp"] && env.is_empty())
    {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "The official Browser manifest does not match the supported host entry point.",
        ));
    }
    Ok(())
}

pub(crate) fn validate_candidate(candidate: &installer::InstallCandidate) -> TalosResult<()> {
    let browser_name = candidate.plugin.metadata.name == BROWSER_MCP_SERVER_NAME
        || matches!(&candidate.plugin.definition, PluginDefinition::McpServer(mcp)
            if mcp.server.name == BROWSER_MCP_SERVER_NAME);
    if !browser_name && candidate.plugin.id.0 != OFFICIAL_BROWSER_PLUGIN_ID {
        return Ok(());
    }
    let expected = talos_config::official_mcp_source();
    if candidate.plugin.id.0 != OFFICIAL_BROWSER_PLUGIN_ID
        || candidate.plugin.kind != PluginKind::McpServer
        || candidate.plugin.path_in_source.as_deref() != Some("mcp/browser")
        || !candidate
            .plugin
            .source_commit
            .as_deref()
            .is_some_and(valid_commit)
        || !matches!(&candidate.source, installer::InstallSource::Marketplace(source)
            if matches!(&source.location, talos_types::MarketplaceSourceLocation::Git {url,reference}
                if *url == expected.url && *reference == expected.reference)
                && source.sparse_paths == expected.sparse_paths)
    {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "The Browser host entry is reserved for its official source.",
        ));
    }
    let PluginDefinition::McpServer(mcp) = &candidate.plugin.definition else {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            "Browser requires an MCP definition.",
        ));
    };
    validate_browser_definition(&mcp.server)
}

pub(crate) fn verified_installed_browser(
    install: &PluginInstall,
) -> TalosResult<(McpServerDefinition, std::path::PathBuf)> {
    if !is_official_browser(install)
        || installer::content_digest_for_dir(&install.installed_path)? != install.content_digest
    {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "The official Browser installation could not be verified.",
        ));
    }
    let server =
        scanner::mcp_manifest::server_from_installed_mcp_manifest(&install.installed_path)?;
    validate_browser_definition(&server)?;
    let configuration = scanner::mcp_manifest::browser_configuration(&install.installed_path)?
        .ok_or_else(|| {
            TalosError::new(
                ErrorCode::InvalidRequest,
                "The Browser dependency configuration is missing.",
            )
        })?;
    let root = std::fs::canonicalize(&install.installed_path).map_err(|error| {
        TalosError::internal_with_source("The installed Browser directory is unavailable.", error)
    })?;
    let lock =
        std::fs::canonicalize(root.join(configuration.docker_requirements)).map_err(|error| {
            TalosError::internal_with_source("The Browser dependency lock is unavailable.", error)
        })?;
    if !lock.is_file() || !lock.starts_with(root) {
        return Err(TalosError::new(
            ErrorCode::PermissionDenied,
            "The Browser dependency lock must remain inside its plugin.",
        ));
    }
    Ok((server, lock))
}

impl PluginManager {
    /// Returns only the dependency lock of the verified installed Browser.
    pub fn installed_browser_requirements(
        &self,
    ) -> TalosResult<Option<(PluginInstall, std::path::PathBuf)>> {
        self.state
            .with_exclusive_lock("plugin.browser.requirements", || {
                let installs = self.state.load_installs()?;
                let Some(install) = installs
                    .installs
                    .iter()
                    .find(|install| is_official_browser(install))
                else {
                    return Ok(None);
                };
                let (_, lock) = verified_installed_browser(install)?;
                Ok(Some((install.clone(), lock)))
            })
    }

    fn publish_official_snapshot(&self, root: &Path) -> TalosResult<()> {
        let configured = talos_config::official_mcp_source();
        let metadata: OfficialSnapshot = talos_fs::read_json(&root.join("source.json"))?;
        if metadata.repository != configured.url || !valid_commit(&metadata.commit) {
            return Err(TalosError::new(
                ErrorCode::PermissionDenied,
                "The packaged MCP snapshot has invalid source metadata.",
            ));
        }
        let id = source::source_id_for(
            &configured.url,
            &configured.reference,
            &configured.sparse_paths,
        );
        let (official, previous) =
            self.state
                .with_exclusive_lock("plugin.snapshot.inspect", || {
                    Ok((
                        self.source_by_id(&id)?,
                        self.state.load_source_scan_cache(&id)?,
                    ))
                })?;
        let directory = super::preparation::StagingDirectory::new(&self.state)?;
        let staging = crate::PluginStateStore::new(directory.path().join("source"));
        let incoming = staging.marketplace_cache_dir(&id).join("repo");
        talos_fs::create_directory(&incoming.join("mcp"), true)?;
        installer::copy_dir(&root.join("mcp"), &incoming.join("mcp"))?;
        let (plugins, warnings) = scanner::scan_materialized_plugins(&official, &incoming)?;
        self.publish_source_scan(
            official,
            previous,
            &directory,
            scanner::SourceScanResult {
                source_commit: Some(metadata.commit),
                plugins,
                warnings,
            },
        )?;
        Ok(())
    }

    /// Uses the existing install API for an absent verified default, without overriding disable.
    pub fn default_browser_install_request(
        &self,
    ) -> TalosResult<Option<talos_types::PluginInstallRequest>> {
        self.state
            .with_exclusive_lock("plugin.browser.default", || {
                if self.state.load_installs()?.installs.iter().any(|install| {
                    is_official_browser(install) || install.name == BROWSER_MCP_SERVER_NAME
                }) {
                    return Ok(None);
                }
                let candidate = self
                    .install_candidate(&talos_types::PluginId(OFFICIAL_BROWSER_PLUGIN_ID.into()))?;
                validate_candidate(&candidate)?;
                let installer::InstallSource::Marketplace(source) = &candidate.source else {
                    return Err(TalosError::internal(
                        "Official Browser requires a marketplace source.",
                    ));
                };
                let root = installer::plugin_source_dir(&self.state, source, &candidate.plugin)?;
                if !scanner::mcp_manifest::requests_default_install(&root)? {
                    return Ok(None);
                }
                Ok(Some(talos_types::PluginInstallRequest {
                    node_id: None,
                    target: talos_types::PluginInstallTarget::Plugin {
                        plugin_id: candidate.plugin.id,
                        expected_source_commit: candidate.plugin.source_commit,
                    },
                    enable: Some(true),
                    conflict_policy: PluginInstallConflictPolicy::Reject,
                }))
            })
    }

    /// Checks the original installation record; no separate Browser enabled state.
    pub fn official_browser_enabled(&self) -> TalosResult<bool> {
        self.state.with_exclusive_lock("plugin.browser.read", || {
            Ok(self
                .state
                .load_installs()?
                .installs
                .iter()
                .any(|install| is_official_browser(install) && install.enabled))
        })
    }

    /// Initializes only the verified default Browser through the existing installer.
    /// The composition root supplies immutable packaged resources. Missing snapshots
    /// leave Browser unavailable; startup never invents a source revision or downloads
    /// an unpinned package. Existing installations, including disable, are preserved.
    pub fn provision_official_browser(
        &self,
        snapshot: Option<&Path>,
    ) -> TalosResult<OfficialBrowserProvision> {
        if let Some(root) = snapshot {
            // Publish one clean tree before the installation transaction; never overlay cache bytes.
            if !self
                .state
                .with_exclusive_lock("plugin.browser.inspect", || {
                    Ok(self
                        .state
                        .load_installs()?
                        .installs
                        .iter()
                        .any(is_official_browser))
                })?
            {
                self.publish_official_snapshot(root)?;
            }
        }
        self.state
            .with_exclusive_lock("plugin.browser.provision", || {
                let previous = self.state.load_installs()?;
                if previous.installs.iter().any(is_official_browser) {
                    return Ok(OfficialBrowserProvision::Ready);
                }
                if previous
                    .installs
                    .iter()
                    .any(|install| install.name == BROWSER_MCP_SERVER_NAME)
                {
                    return Ok(OfficialBrowserProvision::NameConflict);
                }
                let configured = talos_config::official_mcp_source();
                let official = source::config_source(&configured).ok_or_else(|| {
                    TalosError::internal("The official MCP source is unavailable.")
                })?;
                let cache = self.state.load_source_scan_cache(&official.id)?;
                let Some(plugin) = cache
                    .plugins
                    .into_iter()
                    .find(|plugin| plugin.id.0 == OFFICIAL_BROWSER_PLUGIN_ID)
                else {
                    return Ok(OfficialBrowserProvision::Unavailable);
                };
                let root = self
                    .state
                    .marketplace_cache_dir(&official.id)
                    .join("repo/mcp/browser");
                if !scanner::mcp_manifest::requests_default_install(&root)? {
                    return Ok(OfficialBrowserProvision::Unavailable);
                }
                if scanner::mcp_manifest::browser_configuration(&root)?.is_none() {
                    return Err(TalosError::new(
                        ErrorCode::InvalidRequest,
                        "The official Browser dependency configuration is missing.",
                    ));
                }
                let candidate = installer::InstallCandidate {
                    plugin,
                    source: installer::InstallSource::Marketplace(official),
                    prepared_dir: None,
                };
                self.activate_install(candidate, Some(true), PluginInstallConflictPolicy::Reject)?;
                Ok(OfficialBrowserProvision::Ready)
            })
    }
}
