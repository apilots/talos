import { PluginsManagementView } from "./PluginsManagementView";

interface McpPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function McpPluginsManagementView({ query, refreshVersion }: McpPluginsManagementViewProps) {
  return (
    <PluginsManagementView
      catalogLabel="Marketplace MCP servers"
      emptyInstalledDescription="Add a marketplace source or install an MCP server plugin to make it available as model-callable tools."
      installedLabel="Installed MCP servers"
      kind="mcp_server"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}
