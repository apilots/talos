import { useTranslation } from "react-i18next";

import { PluginsManagementView } from "./PluginsManagementView";

interface McpPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function McpPluginsManagementView({ query, refreshVersion }: McpPluginsManagementViewProps) {
  const { t } = useTranslation("plugins");

  return (
    <PluginsManagementView
      catalogLabel={t("views.mcp.catalogLabel")}
      emptyInstalledDescription={t("views.mcp.emptyInstalledDescription")}
      installedLabel={t("views.mcp.installedLabel")}
      kind="mcp_server"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}
