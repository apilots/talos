import type { Plugin } from "@talos/sdk-browser";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { InstalledPluginRow } from "./InstalledPluginRow";
import { MarketplacePluginRow } from "./MarketplacePluginRow";
import { PluginsEmptyState } from "./PluginsEmptyState";

interface PluginManagementPanelProps {
  actionPluginId: string | null;
  catalogLabel?: string;
  emptyInstalledDescription: string;
  installedLabel?: string;
  installedPlugins: Plugin[];
  introSection?: ReactNode;
  loading: boolean;
  marketplacePlugins: Plugin[];
  showCatalog?: boolean;
  warnings: string[];
  onDisable: (plugin: Plugin) => void;
  onEnable: (plugin: Plugin) => void;
  onInstall: (plugin: Plugin) => void;
  onOpen: (plugin: Plugin) => void;
  onUninstall: (plugin: Plugin) => void;
  onUpdate: (plugin: Plugin) => void;
}

export function PluginManagementPanel({
  actionPluginId,
  catalogLabel,
  emptyInstalledDescription,
  installedLabel,
  installedPlugins,
  introSection,
  loading,
  marketplacePlugins,
  onDisable,
  onEnable,
  onInstall,
  onOpen,
  onUninstall,
  onUpdate,
  showCatalog = true,
  warnings,
}: PluginManagementPanelProps) {
  const { t } = useTranslation("plugins");
  const resolvedCatalogLabel = catalogLabel ?? t("views.skills.catalogLabel");
  const resolvedInstalledLabel = installedLabel ?? t("views.skills.installedLabel");

  if (loading && installedPlugins.length === 0 && marketplacePlugins.length === 0) {
    return <div className="mt-10 text-sm text-muted">{t("panel.loading")}</div>;
  }

  return (
    <div className="mt-9 space-y-8">
      {warnings.length > 0 ? (
        <div className="rounded-[0.9rem] border border-border/80 bg-card/70 px-4 py-3 text-sm text-muted">
          {warnings[0]}
        </div>
      ) : null}
      {introSection}
      <section>
        <h2 className="mb-3 text-sm font-semibold text-ink">{resolvedInstalledLabel}</h2>
        {installedPlugins.length === 0 ? (
          <PluginsEmptyState description={emptyInstalledDescription} />
        ) : (
          <div className="space-y-1">
            {installedPlugins.map((plugin) => (
              <InstalledPluginRow
                busy={actionPluginId === plugin.id}
                key={plugin.id}
                onDisable={onDisable}
                onEnable={onEnable}
                onOpen={onOpen}
                onUninstall={onUninstall}
                plugin={plugin}
              />
            ))}
          </div>
        )}
      </section>
      {showCatalog ? (
        <section>
          <h2 className="mb-3 text-sm font-semibold text-ink">{resolvedCatalogLabel}</h2>
          {marketplacePlugins.length === 0 ? (
            <div className="rounded-[1.25rem] border border-dashed border-border/80 bg-card/55 px-6 py-8 text-sm text-muted">
              {t("panel.noMarketplacePlugins")}
            </div>
          ) : (
            <div className="space-y-1">
              {marketplacePlugins.map((plugin) => (
                <MarketplacePluginRow
                  installing={actionPluginId === plugin.id}
                  key={plugin.id}
                  onInstall={onInstall}
                  onOpen={onOpen}
                  onUpdate={onUpdate}
                  plugin={plugin}
                />
              ))}
            </div>
          )}
        </section>
      ) : null}
    </div>
  );
}
