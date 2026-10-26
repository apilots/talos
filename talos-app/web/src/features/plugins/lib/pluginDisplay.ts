import type { McpServerDefinition, Plugin, SkillSummary } from "@talos/sdk-browser";

// Normal Plugin identity derived from the official Git source and mcp/browser path.
export const OFFICIAL_BROWSER_PLUGIN_ID = "plugin_db03ad38c5bfbfed8f9bdd34";

export function isOfficialBrowserPlugin(plugin: Pick<Plugin, "id">) {
  return plugin.id === OFFICIAL_BROWSER_PLUGIN_ID;
}

export function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}

export function pluginTitle(plugin: Plugin) {
  return plugin.metadata.display_name ?? titleFromName(plugin.metadata.name);
}

export function skillTitle(skill: SkillSummary) {
  return skill.metadata?.display_name ?? titleFromName(skill.name);
}

export function pluginSubtitle(plugin: Plugin) {
  if (plugin.kind !== "mcp_server") {
    return plugin.metadata.description;
  }
  const transport = mcpServerDefinition(plugin)?.transport?.type;
  return transport
    ? `${plugin.metadata.description} · ${transport}`
    : plugin.metadata.description;
}

export function mcpServerDefinition(plugin: Plugin): McpServerDefinition | null {
  if (plugin.definition.kind !== "mcp_server") {
    return null;
  }
  return plugin.definition.plugin.server;
}

export function pluginDocumentName(plugin: Plugin, fallback: string) {
  if (plugin.kind === "mcp_server") {
    return "talos-mcp.json";
  }
  if (plugin.kind === "skill") {
    return "SKILL.md";
  }
  return fallback;
}

export function pluginEnabled(plugin: Plugin) {
  if (plugin.status.status === "installed" || plugin.status.status === "update_available") {
    return plugin.status.enabled;
  }
  if (plugin.status.status === "source_missing") {
    return plugin.status.enabled;
  }
  return false;
}

export function installedStatusLabelKey(plugin: Plugin) {
  switch (plugin.status.status) {
    case "installed":
      return plugin.status.enabled ? "enabled" : "disabled";
    case "update_available":
      return "updateAvailable";
    case "source_missing":
      return "sourceMissing";
    case "conflict":
      return "conflict";
    case "not_installed":
      return "notInstalled";
  }
}

export function marketplaceStatusLabelKey(plugin: Plugin) {
  switch (plugin.status.status) {
    case "not_installed":
      return "marketplace";
    case "installed":
      return plugin.status.enabled ? "installed" : "disabled";
    case "update_available":
      return "updateAvailable";
    case "source_missing":
      return "sourceMissing";
    case "conflict":
      return "conflict";
  }
}

export function pluginStatusBadgeClass(plugin: Plugin) {
  const base = "rounded-full border px-2 py-1 text-xs";
  if (plugin.status.status === "update_available") {
    return `${base} border-brand/30 text-brand`;
  }
  if (plugin.status.status === "conflict" || plugin.status.status === "source_missing") {
    return `${base} border-danger/30 text-danger`;
  }
  return `${base} border-border/80 text-muted`;
}

export function stripFrontmatter(contents: string) {
  if (!contents.startsWith("---")) {
    return contents;
  }
  const end = contents.indexOf("\n---", 3);
  if (end < 0) {
    return contents;
  }
  return contents.slice(end + 4).trimStart();
}
