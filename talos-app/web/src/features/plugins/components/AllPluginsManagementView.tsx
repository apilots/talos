import { PluginsManagementView } from "./PluginsManagementView";

interface AllPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function AllPluginsManagementView({ query, refreshVersion }: AllPluginsManagementViewProps) {
  return (
    <PluginsManagementView
      catalogLabel="Marketplace plugins"
      emptyInstalledDescription="Add a marketplace source or install a plugin to make it available."
      installedLabel="Installed plugins"
      kind="all"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}
