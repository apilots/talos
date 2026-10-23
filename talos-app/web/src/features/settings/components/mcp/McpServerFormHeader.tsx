import type { Plugin } from "@talos/sdk-browser";
import { Globe, Power, PowerOff, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { pluginEnabled } from "@/features/plugins/lib/pluginDisplay";

interface McpServerFormHeaderProps {
  busy?: boolean;
  mode: "create" | "update";
  plugin?: Plugin;
  pluginName: string;
  onDisable?: (plugin: Plugin) => void;
  onEnable?: (plugin: Plugin) => void;
  onUninstall?: (plugin: Plugin) => void;
}

export function McpServerFormHeader({
  busy = false,
  mode,
  plugin,
  pluginName,
  onDisable,
  onEnable,
  onUninstall,
}: McpServerFormHeaderProps) {
  const { t } = useTranslation(["plugins", "settings"]);
  const updateMode = mode === "update";
  const title = updateMode
    ? t(pluginName.toLowerCase().endsWith(" mcp") ? "settings:mcpServers.updateTitleBare" : "settings:mcpServers.updateTitle", {
        name: pluginName,
      })
    : t("settings:mcpServers.connectTitle");

  return (
    <header className="mb-8 flex items-start justify-between gap-6">
      <div>
        <h2 className="text-2xl font-semibold tracking-normal text-ink">{title}</h2>
        <a
          className="mt-3 inline-flex items-center gap-1.5 text-sm font-medium text-muted transition hover:text-brand focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
          href="https://modelcontextprotocol.io/docs"
          rel="noreferrer"
          target="_blank"
        >
          {t("settings:mcpServers.docs")}
          <Globe aria-hidden="true" size={14} />
        </a>
      </div>
      {plugin ? (
        <div className="flex items-center gap-2">
          <button
            className="inline-flex h-10 items-center gap-2 rounded-[var(--radius-control)] border border-border/80 px-4 text-sm font-medium text-ink transition hover:bg-hover disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            disabled={busy}
            onClick={() => (pluginEnabled(plugin) ? onDisable?.(plugin) : onEnable?.(plugin))}
            type="button"
          >
            {pluginEnabled(plugin) ? <PowerOff aria-hidden="true" size={15} /> : <Power aria-hidden="true" size={15} />}
            {pluginEnabled(plugin) ? t("plugins:actions.disable") : t("plugins:actions.enable")}
          </button>
          <button
            className="inline-flex h-10 items-center gap-2 rounded-[var(--radius-control)] bg-danger/10 px-4 text-sm font-medium text-danger transition hover:bg-danger/15 disabled:opacity-50 focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/25"
            disabled={busy}
            onClick={() => onUninstall?.(plugin)}
            type="button"
          >
            <Trash2 aria-hidden="true" size={15} />
            {t("plugins:actions.uninstall")}
          </button>
        </div>
      ) : null}
    </header>
  );
}
