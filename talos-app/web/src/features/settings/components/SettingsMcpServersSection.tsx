import { useQuery } from "@tanstack/react-query";
import type { Plugin } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";
import { useState } from "react";

import {
  disablePlugin,
  enablePlugin,
  listManagedMcpPlugins,
  uninstallPlugin,
} from "@/features/plugins/api";
import { mcpServerDefinition, pluginEnabled, pluginTitle } from "@/features/plugins/lib/pluginDisplay";
import { McpServerForm } from "./mcp/McpServerForm";
import { McpServerList, type McpServerView } from "./mcp/McpServerList";
import { McpSourceList, type McpSourceView } from "./mcp/McpSourceList";

type McpFormState =
  | {
      mode: "create";
    }
  | {
      mode: "update";
      plugin: Plugin;
    };

export function SettingsMcpServersSection() {
  const { t } = useTranslation("settings");
  const [formState, setFormState] = useState<McpFormState | null>(null);
  const [actionPluginId, setActionPluginId] = useState<string | null>(null);
  const pluginsQuery = useQuery({
    queryKey: ["talos", "settings", "mcp-servers"],
    queryFn: listManagedMcpPlugins,
  });
  const plugins = pluginsQuery.data?.plugins ?? [];
  const servers = plugins.map(mapServerView);
  const sources = sourceViews(plugins, (count) => t("mcpServers.serverCount", { count }));

  async function runPluginAction(plugin: Plugin, action: (plugin: Plugin) => Promise<unknown>) {
    setActionPluginId(plugin.id);
    try {
      await action(plugin);
      await pluginsQuery.refetch();
    } finally {
      setActionPluginId(null);
    }
  }

  if (formState?.mode === "create") {
    return (
      <McpServerForm
        mode="create"
        onBack={() => setFormState(null)}
      />
    );
  }

  if (formState?.mode === "update") {
    return (
      <McpServerForm
        busy={actionPluginId === formState.plugin.id}
        mode="update"
        onBack={() => setFormState(null)}
        onDisable={(plugin) => void runPluginAction(plugin, disablePlugin)}
        onEnable={(plugin) => void runPluginAction(plugin, enablePlugin)}
        onUninstall={(plugin) => void runPluginAction(plugin, uninstallPlugin)}
        plugin={formState.plugin}
      />
    );
  }

  return (
    <div className="mx-auto max-w-[760px] space-y-10">
      <header className="space-y-2">
        <h2 className="text-2xl font-semibold tracking-normal text-ink text-balance">
          {t("mcpServers.title")}
        </h2>
        <p className="text-sm leading-6 text-muted">
          {t("mcpServers.detail")}{" "}
          <a
            className="font-medium text-brand hover:text-brand/80 focus:outline-none focus-visible:ring-2 focus-visible:ring-brand/30"
            href="https://modelcontextprotocol.io/docs"
            rel="noreferrer"
            target="_blank"
          >
            {t("mcpServers.learnMore")}
          </a>
        </p>
      </header>
      {pluginsQuery.isLoading ? (
        <div className="text-sm text-muted">{t("mcpServers.loading")}</div>
      ) : pluginsQuery.error ? (
        <div className="rounded-[1rem] border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger">
          {pluginsQuery.error instanceof Error ? pluginsQuery.error.message : t("mcpServers.loadError")}
        </div>
      ) : null}
      <McpServerList
        addLabel={t("mcpServers.addServer")}
        actionPluginId={actionPluginId}
        emptyLabel={t("mcpServers.empty")}
        onAdd={() => setFormState({ mode: "create" })}
        onDisable={(plugin) => void runPluginAction(plugin, disablePlugin)}
        onEnable={(plugin) => void runPluginAction(plugin, enablePlugin)}
        onOpen={(plugin) => setFormState({ mode: "update", plugin })}
        onUninstall={(plugin) => void runPluginAction(plugin, uninstallPlugin)}
        servers={servers}
        title={t("mcpServers.servers")}
      />
      <McpSourceList sources={sources} title={t("mcpServers.fromPlugins")} />
    </div>
  );
}

function mapServerView(plugin: Plugin): McpServerView {
  const definition = mcpServerDefinition(plugin);
  return {
    id: plugin.id,
    enabled: pluginEnabled(plugin),
    name: definition?.display_name ?? definition?.name ?? pluginTitle(plugin),
    plugin,
    sourceId: plugin.source_id,
    transport: definition?.transport.type ?? null,
  };
}

function sourceViews(plugins: Plugin[], formatCount: (count: number) => string): McpSourceView[] {
  const counts = new Map<string, number>();
  for (const plugin of plugins) {
    const sourceId = plugin.source_id ?? "manual";
    counts.set(sourceId, (counts.get(sourceId) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([id, count]) => ({
      id,
      name: id,
      count: formatCount(count),
    }));
}
