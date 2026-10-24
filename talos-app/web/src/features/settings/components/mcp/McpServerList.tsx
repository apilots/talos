import type { Plugin, McpServerInfo } from "@talos/sdk-browser";
import { Plus, Settings, ToggleLeft, ToggleRight, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { cn } from "@/shared/lib/cn";

export interface McpServerView {
  connection?: McpServerInfo;
  id: string;
  enabled: boolean;
  name: string;
  plugin: Plugin;
  sourceId: string | null;
  transport: string | null;
}

interface McpServerListProps {
  actionPluginId: string | null;
  addLabel: string;
  emptyLabel: string;
  servers: McpServerView[];
  title: string;
  onAdd: () => void;
  onDisable: (plugin: Plugin) => void;
  onEnable: (plugin: Plugin) => void;
  onOpen: (plugin: Plugin) => void;
  onUninstall: (plugin: Plugin) => void;
}

export function McpServerList({
  actionPluginId,
  addLabel,
  emptyLabel,
  onAdd,
  onDisable,
  onEnable,
  onOpen,
  onUninstall,
  servers,
  title,
}: McpServerListProps) {
  return (
    <section>
      <div className="mb-4 flex items-center justify-between gap-4">
        <h3 className="text-sm font-semibold text-ink">{title}</h3>
        <button
          className="inline-flex h-9 items-center gap-2 rounded-[var(--radius-control)] border border-border/80 bg-card/50 px-3 text-sm font-medium text-ink transition hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
          onClick={onAdd}
          type="button"
        >
          <Plus aria-hidden="true" size={16} />
          {addLabel}
        </button>
      </div>
      {servers.length === 0 ? (
        <div className="rounded-[1.05rem] border border-dashed border-border/80 bg-card/45 px-5 py-8 text-sm text-muted">
          {emptyLabel}
        </div>
      ) : (
        <div className="overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45">
          {servers.map((server) => (
            <McpServerRow
              busy={actionPluginId === server.id}
              key={server.id}
              onDisable={() => onDisable(server.plugin)}
              onEnable={() => onEnable(server.plugin)}
              onOpen={() => onOpen(server.plugin)}
              onUninstall={() => onUninstall(server.plugin)}
              server={server}
            />
          ))}
        </div>
      )}
    </section>
  );
}

function McpServerRow({
  busy,
  onDisable,
  onEnable,
  onOpen,
  onUninstall,
  server,
}: {
  busy: boolean;
  onDisable: () => void;
  onEnable: () => void;
  onOpen: () => void;
  onUninstall: () => void;
  server: McpServerView;
}) {
  const { t } = useTranslation(["plugins", "settings"]);
  const builtin = server.id === "builtin_browser";
  return (
    <div className={cn("grid min-h-[4.5rem] items-center gap-4 border-b border-border/65 px-5 last:border-b-0 max-sm:py-3", builtin ? "grid-cols-[minmax(0,1fr)_max-content_max-content] max-sm:grid-cols-[minmax(0,1fr)_max-content]" : "grid-cols-[minmax(0,1fr)_max-content_max-content_max-content_max-content] max-sm:grid-cols-3")}>
      <div className={cn("min-w-0",!builtin&&"max-sm:col-span-3")}>
        <span className="block truncate text-sm font-medium text-ink">{server.name}</span>
        <span className="mt-0.5 block truncate text-xs text-muted max-sm:whitespace-normal max-sm:break-words">
          {server.id === "builtin_browser" ? t("settings:mcpServers.browser.builtIn") : server.transport ?? t("settings:mcpServers.unknownTransport")}
          {server.connection ? ` · ${t(`settings:mcpServers.connectionState.${server.connection.status.status}`)} · ${t("settings:mcpServers.toolsCount", { count: server.connection.toolCount })}` : ""}
        </span>
        {server.connection && "error" in server.connection.status ? <p className="mt-1 text-xs text-danger">{apiErrorMessage(server.connection.status.error)}</p> : null}
      </div>
      <span className={cn("inline-flex items-center gap-2 text-sm text-muted",builtin?"max-sm:hidden":"max-sm:col-span-3")}>
        <span className={cn("size-1.5 rounded-full", server.enabled ? "bg-success" : "bg-muted/60")} />
        {server.enabled ? t("plugins:status.enabled") : t("plugins:status.disabled")}
      </span>
      {!builtin ? <button
        aria-label={t("settings:mcpServers.openPlugin")}
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-hover hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
        disabled={busy}
        onClick={onOpen}
        type="button"
      >
        <Settings aria-hidden="true" size={16} />
      </button> : null}
      <button
        aria-label={server.enabled ? t("plugins:actions.disable") : t("plugins:actions.enable")}
        className={cn(
          "flex size-9 items-center justify-center rounded-[var(--radius-control)] transition focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30",
          server.enabled ? "text-brand hover:bg-hover" : "text-muted hover:bg-hover hover:text-ink",
        )}
        disabled={busy}
        onClick={server.enabled ? onDisable : onEnable}
        type="button"
      >
        {server.enabled ? <ToggleRight aria-hidden="true" size={28} /> : <ToggleLeft aria-hidden="true" size={28} />}
      </button>
      {!builtin ? <button
        aria-label={t("plugins:actions.uninstall")}
        className="flex size-8 items-center justify-center rounded-[var(--radius-control)] text-muted transition hover:bg-danger/10 hover:text-danger focus:outline-none focus-visible:ring-2 focus-visible:ring-danger/25"
        disabled={busy}
        onClick={onUninstall}
        type="button"
      >
        <Trash2 aria-hidden="true" size={16} />
      </button> : null}
    </div>
  );
}
