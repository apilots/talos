import { Cloud } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";

interface MarketplaceSkillRowProps {
  plugin: Plugin;
}

export function MarketplaceSkillRow({ plugin }: MarketplaceSkillRowProps) {
  return (
    <div className="grid w-full grid-cols-[44px_minmax(0,1fr)_max-content] items-center gap-3 rounded-[0.8rem] px-2 py-2.5 text-left">
      <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
        <Cloud size={18} />
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold text-ink">
          {marketplaceTitle(plugin)}
        </div>
        <div className="mt-0.5 truncate text-sm text-muted">{plugin.description}</div>
      </div>
      <span className="rounded-full border border-border/80 px-2 py-1 text-xs text-muted">
        Marketplace
      </span>
    </div>
  );
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
