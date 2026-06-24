import type { Plugin } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import {
  installedStatusLabelKey,
  marketplaceStatusLabelKey,
  pluginStatusBadgeClass,
} from "@/features/plugins/lib/pluginDisplay";

interface PluginStatusBadgeProps {
  plugin: Plugin;
  variant?: "installed" | "marketplace";
}

export function PluginStatusBadge({ plugin, variant = "installed" }: PluginStatusBadgeProps) {
  const { t } = useTranslation("plugins");
  const labelKey = variant === "marketplace"
    ? marketplaceStatusLabelKey(plugin)
    : installedStatusLabelKey(plugin);
  const label = t(`status.${labelKey}`);
  return <span className={pluginStatusBadgeClass(plugin)}>{label}</span>;
}
