import { type ReactNode, useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { Plugin, PluginSearchRefreshPolicy } from "@talos/sdk-browser";

import {
  disablePlugin,
  enablePlugin,
  installPlugin,
  listManagedMcpPlugins,
  listManagedPlugins,
  listManagedSkillPlugins,
  searchMcpPlugins,
  searchPlugins,
  searchSkillPlugins,
  uninstallPlugin,
  updatePlugin,
} from "@/features/plugins/api";
import { PluginDocumentDialog } from "./PluginDocumentDialog";
import { PluginManagementPanel } from "./PluginManagementPanel";

type PluginKindFilter = "all" | "skill" | "mcp_server";

interface PluginsManagementViewProps {
  catalogLabel: string;
  emptyInstalledDescription: string;
  installedLabel: string;
  introSection?: ReactNode;
  kind: PluginKindFilter;
  query: string;
  refreshVersion: number;
}

export function PluginsManagementView({
  catalogLabel,
  emptyInstalledDescription,
  installedLabel,
  introSection,
  kind,
  query,
  refreshVersion,
}: PluginsManagementViewProps) {
  const [selectedPlugin, setSelectedPlugin] = useState<Plugin | null>(null);
  const [actionPluginId, setActionPluginId] = useState<string | null>(null);
  const installedPluginsQuery = useQuery({
    queryKey: ["talos", "plugins", "installed", kind],
    queryFn: () => listManagedPluginsForKind(kind),
  });
  const marketplaceQuery = useQuery({
    queryKey: ["talos", "plugins", kind, "search", query],
    queryFn: () => searchPluginsForKind(kind, query, "refresh_stale"),
  });
  const marketplacePlugins = useMemo(
    () => marketplaceQuery.data?.plugins ?? [],
    [marketplaceQuery.data?.plugins],
  );
  const installedPlugins = useMemo(
    () => installedPluginsQuery.data?.plugins ?? [],
    [installedPluginsQuery.data?.plugins],
  );

  const refresh = useCallback(async () => {
    await Promise.all([
      installedPluginsQuery.refetch(),
      searchPluginsForKind(kind, query, "force_refresh").then(() => marketplaceQuery.refetch()),
    ]);
  }, [installedPluginsQuery.refetch, kind, marketplaceQuery.refetch, query]);

  useEffect(() => {
    if (refreshVersion === 0) {
      return;
    }
    void refresh();
  }, [refresh, refreshVersion]);

  async function runPluginAction(plugin: Plugin, action: (plugin: Plugin) => Promise<unknown>) {
    setActionPluginId(plugin.id);
    try {
      await action(plugin);
      await Promise.all([
        installedPluginsQuery.refetch(),
        marketplaceQuery.refetch(),
      ]);
    } finally {
      setActionPluginId(null);
    }
  }

  return (
    <>
      <PluginManagementPanel
        actionPluginId={actionPluginId}
        catalogLabel={catalogLabel}
        emptyInstalledDescription={emptyInstalledDescription}
        installedLabel={installedLabel}
        installedPlugins={installedPlugins}
        introSection={introSection}
        loading={installedPluginsQuery.isLoading || marketplaceQuery.isLoading}
        marketplacePlugins={marketplacePlugins}
        onDisable={(plugin) => void runPluginAction(plugin, disablePlugin)}
        onEnable={(plugin) => void runPluginAction(plugin, enablePlugin)}
        onInstall={(plugin) => void runPluginAction(plugin, installPlugin)}
        onOpen={setSelectedPlugin}
        onUninstall={(plugin) => void runPluginAction(plugin, uninstallPlugin)}
        onUpdate={(plugin) => void runPluginAction(plugin, updatePlugin)}
        warnings={warningMessages(marketplaceQuery.data?.warnings ?? [])}
      />
      <PluginDocumentDialog onClose={() => setSelectedPlugin(null)} plugin={selectedPlugin} />
    </>
  );
}

function listManagedPluginsForKind(kind: PluginKindFilter) {
  switch (kind) {
    case "skill":
      return listManagedSkillPlugins();
    case "mcp_server":
      return listManagedMcpPlugins();
    case "all":
      return listManagedPlugins();
  }
}

function searchPluginsForKind(
  kind: PluginKindFilter,
  query: string,
  refresh: PluginSearchRefreshPolicy,
) {
  switch (kind) {
    case "skill":
      return searchSkillPlugins(query, refresh);
    case "mcp_server":
      return searchMcpPlugins(query, refresh);
    case "all":
      return searchPlugins(query, refresh);
  }
}

function warningMessages(warnings: Array<{ message: string }>) {
  return warnings.map((warning) => warning.message);
}
