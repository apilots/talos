import { getTalosClient } from "@/shared/api/client";
import type {
  MarketplaceSourceInput,
  PluginSearchRefreshPolicy,
  ProtocolSkillSummary,
} from "@talos/sdk-browser";

export async function listSkills(includeDisabled = true) {
  return getTalosClient().skills.list({ include_disabled: includeDisabled });
}

export async function readSkillDocument(skill: ProtocolSkillSummary) {
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
    status: "not_installed",
    refresh,
    limit: 80,
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
