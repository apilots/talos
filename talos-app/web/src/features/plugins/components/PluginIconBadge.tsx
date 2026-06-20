import { Box, Cloud, FileText, Server } from "lucide-react";
import type { Plugin } from "@talos/sdk-browser";

interface PluginIconBadgeProps {
  fallback?: "box" | "cloud" | "file";
  plugin: Plugin;
}

export function PluginIconBadge({ fallback = "box", plugin }: PluginIconBadgeProps) {
  const icon = plugin.metadata.icon?.trim();
  return (
    <div className="flex size-10 items-center justify-center rounded-[0.7rem] bg-card text-brand">
      {icon ? (
        <PluginIconValue value={icon} />
      ) : plugin.kind === "mcp_server" ? (
        <Server size={18} />
      ) : fallback === "cloud" ? (
        <Cloud size={18} />
      ) : fallback === "file" ? (
        <FileText size={18} />
      ) : (
        <Box size={18} />
      )}
    </div>
  );
}

function PluginIconValue({ value }: { value: string }) {
  if (value.startsWith("data:image/") || value.startsWith("http://") || value.startsWith("https://")) {
    return <img alt="" className="size-5 object-contain" src={value} />;
  }
  return <span className="text-sm font-semibold">{value.slice(0, 2).toUpperCase()}</span>;
}
