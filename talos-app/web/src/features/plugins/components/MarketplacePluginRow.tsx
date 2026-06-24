import type { Plugin } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { PluginIconBadge } from "./PluginIconBadge";
import { PluginStatusBadge } from "./PluginStatusBadge";
import { pluginSubtitle, pluginTitle } from "@/features/plugins/lib/pluginDisplay";

interface MarketplacePluginRowProps {
  plugin: Plugin;
  installing?: boolean;
  onInstall?: (plugin: Plugin) => void;
  onOpen?: (plugin: Plugin) => void;
  onUpdate?: (plugin: Plugin) => void;
}

export function MarketplacePluginRow({
  installing,
  onInstall,
  onOpen,
  onUpdate,
  plugin,
}: MarketplacePluginRowProps) {
  const { t } = useTranslation("plugins");
  const action = marketplaceAction(plugin, onInstall, onUpdate, t);
  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5 text-left">
      <PluginIconBadge fallback="cloud" plugin={plugin} />
      <div className="min-w-0">
        <button
          className="block max-w-full truncate text-left text-sm font-semibold text-ink transition hover:text-brand focus:outline-none focus:ring-2 focus:ring-brand/30"
          onClick={() => onOpen?.(plugin)}
          type="button"
        >
          {pluginTitle(plugin)}
        </button>
        <div className="mt-0.5 truncate text-sm text-muted">{pluginSubtitle(plugin)}</div>
      </div>
      {action ? (
        <button
          className={action.className}
          disabled={installing || action.disabled}
          onClick={() => action.onClick(plugin)}
          type="button"
        >
          {installing ? action.busyLabel : action.label}
        </button>
      ) : (
        <PluginStatusBadge plugin={plugin} variant="marketplace" />
      )}
    </div>
  );
}

function marketplaceAction(
  plugin: Plugin,
  onInstall?: (plugin: Plugin) => void,
  onUpdate?: (plugin: Plugin) => void,
  t: (key: string) => string = (key) => key,
) {
  if (plugin.status.status === "not_installed" && onInstall) {
    return {
      label: t("actions.install"),
      busyLabel: t("actions.installing"),
      disabled: false,
      onClick: onInstall,
      className:
        "rounded-full bg-ink px-3 py-1.5 text-xs font-medium text-background transition disabled:bg-muted/40",
    };
  }
  if (plugin.status.status === "update_available" && onUpdate) {
    return {
      label: t("actions.update"),
      busyLabel: t("actions.updating"),
      disabled: false,
      onClick: onUpdate,
      className:
        "rounded-full bg-brand px-3 py-1.5 text-xs font-medium text-background transition disabled:bg-muted/40",
    };
  }
  if (plugin.status.status === "installed") {
    return {
      label: plugin.status.enabled ? t("status.installed") : t("status.disabled"),
      busyLabel: plugin.status.enabled ? t("status.installed") : t("status.disabled"),
      disabled: true,
      onClick: () => undefined,
      className:
        "rounded-full border border-border/80 px-3 py-1.5 text-xs font-medium text-muted transition disabled:opacity-80",
    };
  }
  return null;
}
