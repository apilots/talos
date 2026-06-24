import { getTalosClient } from "@/shared/api/client";
import type {
  MarketplaceSourceInput,
  Plugin,
  PluginSearchRefreshPolicy,
  SkillSummary,
} from "@talos/sdk-browser";

type PluginKind = Plugin["kind"];

export async function listSkills(includeDisabled = true) {
  return getTalosClient().skills.list({ include_disabled: includeDisabled });
}

export async function readLocalSkill(skill: SkillSummary) {
  return getTalosClient().skills.read({
    selector: {
      name: skill.name,
      path: skill.path,
    },
  });
}

export async function searchSkillPlugins(
  query: string,
  refresh: PluginSearchRefreshPolicy = "refresh_stale",
) {
  return searchPluginsByKind("skill", query, refresh);
}

export async function searchMcpPlugins(
  query: string,
  refresh: PluginSearchRefreshPolicy = "refresh_stale",
) {
  return searchPluginsByKind("mcp_server", query, refresh);
}

export async function searchPlugins(
  query: string,
  refresh: PluginSearchRefreshPolicy = "refresh_stale",
) {
  return searchPluginsByKind(null, query, refresh);
}

export async function searchPluginsByKind(
  kind: PluginKind | null,
  query: string,
  refresh: PluginSearchRefreshPolicy = "refresh_stale",
) {
  return getTalosClient().plugin.search({
    query: query.trim() || null,
    source_id: null,
    kind,
    status: "any",
    refresh,
    limit: 80,
  });
}

export async function listManagedSkillPlugins() {
  return listManagedPluginsByKind("skill");
}

export async function listManagedMcpPlugins() {
  return listManagedPluginsByKind("mcp_server");
}

export async function listManagedPlugins() {
  return listManagedPluginsByKind(null);
}

export async function listManagedPluginsByKind(kind: PluginKind | null) {
  return getTalosClient().plugin.list({
    kind,
    status: null,
    source_id: null,
    include_disabled: true,
  });
}

export async function readPluginDocument(plugin: Plugin) {
  return getTalosClient().plugin.read({ plugin_id: plugin.id });
}

export async function installPlugin(plugin: Plugin) {
  return getTalosClient().plugin.install({
    target: {
      kind: "plugin",
      plugin_id: plugin.id,
      expected_source_commit: plugin.source_commit,
    },
    enable: true,
    conflict_policy: "reject",
  });
}

export async function enablePlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.enable({ install_id: installId });
}

export async function disablePlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.disable({ install_id: installId });
}

export async function uninstallPlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.uninstall({ install_id: installId });
}

export async function updatePlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.update({
    install_id: installId,
    expected_source_commit: plugin.source_commit,
    conflict_policy: "reject_local_changes",
  });
}

export async function addMarketplaceSource(input: AddMarketplaceSourceInput) {
  return getTalosClient().plugin.sources.add({
    source: marketplaceSourceInput(input),
    refresh: true,
  });
}

export interface AddMarketplaceSourceInput {
  source: string;
  gitRef: string;
  sparsePaths: string;
}

function marketplaceSourceInput(input: AddMarketplaceSourceInput): MarketplaceSourceInput {
  const source = input.source.trim();
  const sparsePaths = input.sparsePaths
    .split(/\r?\n|,/)
    .map((path) => path.trim())
    .filter(Boolean);
  if (source.startsWith("/") || source.startsWith("~")) {
    return {
      kind: "local_folder",
      path: source,
      sparse_paths: sparsePaths,
    };
  }
  return {
    kind: "git",
    url: normalizeGitSource(source),
    reference: input.gitRef.trim() || "main",
    sparse_paths: sparsePaths.length > 0 ? sparsePaths : ["skills"],
  };
}

function normalizeGitSource(source: string) {
  if (source.startsWith("https://") || source.startsWith("git@")) {
    return source;
  }
  if (/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(source)) {
    return `https://github.com/${source}.git`;
  }
  return source;
}

function pluginInstallId(plugin: Plugin) {
  return "install_id" in plugin.status ? plugin.status.install_id : null;
}
