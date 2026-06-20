import { getTalosClient } from "@/shared/api/client";
import type {
  MarketplaceSourceInput,
  Plugin,
  PluginSearchRefreshPolicy,
  SkillSummary,
} from "@talos/sdk-browser";

export async function listSkills(includeDisabled = true) {
  return getTalosClient().skills.list({ include_disabled: includeDisabled });
}

export async function readSkillDocument(skill: SkillSummary) {
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
  return getTalosClient().plugin.search({
    query: query.trim() || null,
    source_id: null,
    kind: "skill",
    status: "any",
    refresh,
    limit: 80,
  });
}

export async function listManagedSkillPlugins() {
  return getTalosClient().plugin.list({
    kind: "skill",
    status: null,
    source_id: null,
    include_disabled: true,
  });
}

export async function readPluginDocument(plugin: Plugin) {
  return getTalosClient().plugin.read({ plugin_id: plugin.id });
}

export async function installSkillPlugin(plugin: Plugin) {
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

export async function enableSkillPlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.enable({ install_id: installId });
}

export async function disableSkillPlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.disable({ install_id: installId });
}

export async function uninstallSkillPlugin(plugin: Plugin) {
  const installId = pluginInstallId(plugin);
  if (!installId) {
    throw new Error("Plugin is not installed.");
  }
  return getTalosClient().plugin.uninstall({ install_id: installId });
}

export async function updateSkillPlugin(plugin: Plugin) {
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
