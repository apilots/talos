import { Box, Power, PowerOff, Trash2 } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";

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
  const enabled = plugin.status.status === "installed" || plugin.status.status === "update_available"
    ? plugin.status.enabled
    : plugin.status.status === "source_missing"
      ? plugin.status.enabled
      : false;

  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5">
      <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
        <Box size={18} />
      </div>
      <div className="min-w-0">
        <button
          className="block max-w-full truncate text-left text-sm font-semibold text-ink transition hover:text-brand focus:outline-none focus:ring-2 focus:ring-brand/30"
          onClick={() => onOpen(plugin)}
          type="button"
        >
          {titleFromName(plugin.name)}
        </button>
        <div className="mt-0.5 truncate text-sm text-muted">{plugin.description}</div>
      </div>
      <div className="flex items-center gap-1.5">
        <span className={statusBadgeClass(plugin)}>
          {statusLabel(plugin)}
        </span>
        <button
          className="inline-flex size-8 items-center justify-center rounded-full border border-border/80 text-muted transition hover:border-brand/35 hover:text-ink disabled:opacity-50"
          disabled={busy}
          onClick={() => (enabled ? onDisable(plugin) : onEnable(plugin))}
          title={enabled ? "Disable" : "Enable"}
          type="button"
        >
          {enabled ? <PowerOff size={14} /> : <Power size={14} />}
        </button>
        <button
          className="inline-flex size-8 items-center justify-center rounded-full border border-border/80 text-muted transition hover:border-danger/45 hover:text-danger disabled:opacity-50"
          disabled={busy}
          onClick={() => onUninstall(plugin)}
          title="Uninstall"
          type="button"
        >
          <Trash2 size={14} />
        </button>
      </div>
    </div>
  );
}

function statusLabel(plugin: Plugin) {
  switch (plugin.status.status) {
    case "installed":
      return plugin.status.enabled ? "Enabled" : "Disabled";
    case "update_available":
      return "Update available";
    case "source_missing":
      return "Source missing";
    case "conflict":
      return "Conflict";
    case "not_installed":
      return "Not installed";
  }
}

function statusBadgeClass(plugin: Plugin) {
  const base = "rounded-full border px-2 py-1 text-xs";
  if (plugin.status.status === "installed" && plugin.status.enabled) {
    return `${base} border-border/80 text-muted`;
  }
  if (plugin.status.status === "installed") {
    return `${base} border-border/80 text-muted`;
  }
  if (plugin.status.status === "update_available") {
    return `${base} border-brand/30 text-brand`;
  }
  return `${base} border-danger/30 text-danger`;
}

function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}
