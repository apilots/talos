import { useTranslation } from "react-i18next";

import { PluginsManagementView } from "./PluginsManagementView";

interface AllPluginsManagementViewProps {
  query: string;
  refreshVersion: number;
}

export function AllPluginsManagementView({ query, refreshVersion }: AllPluginsManagementViewProps) {
  const { t } = useTranslation("plugins");

  return (
    <PluginsManagementView
      catalogLabel={t("views.all.catalogLabel")}
      emptyInstalledDescription={t("views.all.emptyInstalledDescription")}
      installedLabel={t("views.all.installedLabel")}
      kind="all"
      query={query}
      refreshVersion={refreshVersion}
    />
  );
}
