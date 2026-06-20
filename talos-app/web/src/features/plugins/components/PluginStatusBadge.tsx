import type { Plugin } from "@talos/sdk-browser";

import {
  installedStatusLabel,
  marketplaceStatusLabel,
  pluginStatusBadgeClass,
} from "@/features/plugins/lib/pluginDisplay";

interface PluginStatusBadgeProps {
  plugin: Plugin;
  variant?: "installed" | "marketplace";
}

export function PluginStatusBadge({ plugin, variant = "installed" }: PluginStatusBadgeProps) {
  const label = variant === "marketplace"
    ? marketplaceStatusLabel(plugin)
    : installedStatusLabel(plugin);
  return <span className={pluginStatusBadgeClass(plugin)}>{label}</span>;
}
