import { Power, PowerOff, Trash2 } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { PluginIconBadge } from "./PluginIconBadge";
import { PluginStatusBadge } from "./PluginStatusBadge";
import { pluginEnabled, pluginSubtitle, pluginTitle } from "@/features/plugins/lib/pluginDisplay";

interface InstalledPluginRowProps {
  busy?: boolean;
  plugin: Plugin;
  onDisable: (plugin: Plugin) => void;
  onEnable: (plugin: Plugin) => void;
  onOpen: (plugin: Plugin) => void;
  onUninstall: (plugin: Plugin) => void;
}

export function InstalledPluginRow({
  busy,
  onDisable,
  onEnable,
  onOpen,
  onUninstall,
  plugin,
}: InstalledPluginRowProps) {
  const { t } = useTranslation("plugins");
  const enabled = pluginEnabled(plugin);

  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5">
      <PluginIconBadge plugin={plugin} />
      <div className="min-w-0">
        <button
          className="block max-w-full truncate text-left text-sm font-semibold text-ink transition hover:text-brand focus:outline-none focus:ring-2 focus:ring-brand/30"
          onClick={() => onOpen(plugin)}
          type="button"
        >
          {pluginTitle(plugin)}
        </button>
        <div className="mt-0.5 truncate text-sm text-muted">{pluginSubtitle(plugin)}</div>
      </div>
      <div className="flex items-center gap-1.5">
        <PluginStatusBadge plugin={plugin} />
        <button
          className="inline-flex size-8 items-center justify-center rounded-full border border-border/80 text-muted transition hover:border-brand/35 hover:text-ink disabled:opacity-50"
          disabled={busy}
          onClick={() => (enabled ? onDisable(plugin) : onEnable(plugin))}
          title={enabled ? t("actions.disable") : t("actions.enable")}
          type="button"
        >
          {enabled ? <PowerOff size={14} /> : <Power size={14} />}
        </button>
        <button
          className="inline-flex size-8 items-center justify-center rounded-full border border-border/80 text-muted transition hover:border-danger/45 hover:text-danger disabled:opacity-50"
          disabled={busy}
          onClick={() => onUninstall(plugin)}
          title={t("actions.uninstall")}
          type="button"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}
