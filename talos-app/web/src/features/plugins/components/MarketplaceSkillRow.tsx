import { Cloud } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";

interface MarketplaceSkillRowProps {
  plugin: Plugin;
  installing?: boolean;
  onInstall?: (plugin: Plugin) => void;
  onOpen?: (plugin: Plugin) => void;
  onUpdate?: (plugin: Plugin) => void;
}

export function MarketplaceSkillRow({
  installing,
  onInstall,
  onOpen,
  onUpdate,
  plugin,
}: MarketplaceSkillRowProps) {
  const action = marketplaceAction(plugin, onInstall, onUpdate);
  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5 text-left">
      <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
        <Cloud size={18} />
      </div>
      <div className="min-w-0">
        <button
          className="block max-w-full truncate text-left text-sm font-semibold text-ink transition hover:text-brand focus:outline-none focus:ring-2 focus:ring-brand/30"
          onClick={() => onOpen?.(plugin)}
          type="button"
        >
          {marketplaceTitle(plugin)}
        </button>
        <div className="mt-0.5 truncate text-sm text-muted">{plugin.description}</div>
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
        <span className={statusBadgeClass(plugin)}>
          {marketplaceStatusLabel(plugin)}
        </span>
      )}
    </div>
  );
}

function marketplaceAction(
  plugin: Plugin,
  onInstall?: (plugin: Plugin) => void,
  onUpdate?: (plugin: Plugin) => void,
) {
  if (plugin.status.status === "not_installed" && onInstall) {
    return {
      label: "Install",
      busyLabel: "Installing",
      disabled: false,
      onClick: onInstall,
      className:
        "rounded-full bg-ink px-3 py-1.5 text-xs font-medium text-background transition disabled:bg-muted/40",
    };
  }
  if (plugin.status.status === "update_available" && onUpdate) {
    return {
      label: "Update",
      busyLabel: "Updating",
      disabled: false,
      onClick: onUpdate,
      className:
        "rounded-full bg-brand px-3 py-1.5 text-xs font-medium text-background transition disabled:bg-muted/40",
    };
  }
  if (plugin.status.status === "installed") {
    return {
      label: plugin.status.enabled ? "Installed" : "Disabled",
      busyLabel: plugin.status.enabled ? "Installed" : "Disabled",
      disabled: true,
      onClick: () => undefined,
      className:
        "rounded-full border border-border/80 px-3 py-1.5 text-xs font-medium text-muted transition disabled:opacity-80",
    };
  }
  return null;
}

function marketplaceStatusLabel(plugin: Plugin) {
  switch (plugin.status.status) {
    case "not_installed":
      return "Marketplace";
    case "installed":
      return plugin.status.enabled ? "Installed" : "Disabled";
    case "update_available":
      return "Update available";
    case "source_missing":
      return "Source missing";
    case "conflict":
      return "Conflict";
  }
}

function statusBadgeClass(plugin: Plugin) {
  const base = "rounded-full border px-2 py-1 text-xs";
  if (plugin.status.status === "update_available") {
    return `${base} border-brand/30 text-brand`;
  }
  if (plugin.status.status === "conflict" || plugin.status.status === "source_missing") {
    return `${base} border-danger/30 text-danger`;
  }
  return `${base} border-border/80 text-muted`;
}

function marketplaceTitle(plugin: Plugin) {
  if (plugin.definition.kind === "skill") {
    return plugin.definition.plugin.manifest.metadata?.display_name ?? titleFromName(plugin.name);
  }
  return titleFromName(plugin.name);
}

function titleFromName(name: string) {
  return name
    .split(/[-_]/)
    .filter(Boolean)
    .map((part) => `${part.charAt(0).toUpperCase()}${part.slice(1)}`)
    .join(" ");
}
