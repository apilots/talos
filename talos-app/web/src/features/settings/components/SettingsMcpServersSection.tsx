import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { useQuery } from "@tanstack/react-query";
import type { Plugin, McpServerInfo } from "@talos/sdk-browser";
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

import { getTalosClient } from "@/shared/api/client";
import { McpCredentialsForm } from "./mcp/McpCredentialsForm";

type McpFormState =
  | {
      mode: "create";
    }
  | {
      mode: "update";
      pluginId: string;
    };

export function SettingsMcpServersSection() {
  const { t } = useTranslation("settings");
  const [actionError, setActionError] = useState<unknown>(null);
  const [configOpen, setConfigOpen] = useState<string | null>(null);
  const [formState, setFormState] = useState<McpFormState | null>(null);
  const [actionPluginId, setActionPluginId] = useState<string | null>(null);
  const pluginsQuery = useQuery({
    queryKey: ["talos", "settings", "mcp-servers"],
    queryFn: listManagedMcpPlugins,
  });
  const connectionsQuery = useQuery({
    queryKey: ["talos", "settings", "mcp-connections"],
    queryFn: async () => {
      const servers: McpServerInfo[] = [];
      let cursor: string | undefined;
      do { const page = await getTalosClient().mcp.list({ cursor, limit: 200 }); servers.push(...page.data); cursor = page.nextCursor ?? undefined; } while (cursor);
      return servers;
    },
    refetchInterval: 2000,
  });
  const connections = connectionsQuery.data ?? [];
  const plugins = pluginsQuery.data?.plugins ?? [];
  const servers = plugins.map((plugin) => ({ ...mapServerView(plugin), connection: connections.find((server) => server.source.kind === "plugin" && "install_id" in plugin.status && server.source.installId === plugin.status.install_id) }));
  async function saved() { await pluginsQuery.refetch(); await connectionsQuery.refetch(); setFormState(null); }
  const sources = sourceViews(plugins, (count) => t("mcpServers.serverCount", { count }));

  async function runPluginAction(plugin: Plugin, action: (plugin: Plugin) => Promise<unknown>) {
    setActionError(null);
    setActionPluginId(plugin.id);
    try {
      await action(plugin);
      await saved();
    } catch (error) {
      setActionError(error);
      setFormState(null);
    } finally {
      setActionPluginId(null);
    }
  }

  if (formState?.mode === "create") {
    return (
      <McpServerForm
        mode="create"
        onSaved={saved}
        onBack={() => setFormState(null)}
      />
    );
  }

  const editingPlugin = formState?.mode === "update" ? plugins.find((plugin) => plugin.id === formState.pluginId) : undefined;
  if (formState?.mode === "update" && editingPlugin) {
    return (
      <McpServerForm
        busy={actionPluginId === editingPlugin.id}
        mode="update"
        onSaved={saved}
        onBack={() => setFormState(null)}
        onDisable={(plugin) => void runPluginAction(plugin, disablePlugin)}
        onEnable={(plugin) => void runPluginAction(plugin, enablePlugin)}
        onUninstall={(plugin) => void runPluginAction(plugin, uninstallPlugin)}
        plugin={editingPlugin}
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
          {userVisibleErrorMessage(pluginsQuery.error, t("mcpServers.loadError"))}
        </div>
      ) : null}
      {actionError ? <p className="text-sm text-danger" role="alert">{userVisibleErrorMessage(actionError, t("mcpServers.saveError"))}</p> : null}
      {connectionsQuery.error ? <p className="text-sm text-danger" role="alert">{userVisibleErrorMessage(connectionsQuery.error, t("mcpServers.loadError"))}</p> : null}
      <McpServerList
        addLabel={t("mcpServers.addServer")}
        actionPluginId={actionPluginId}
        emptyLabel={t("mcpServers.empty")}
        onAdd={() => setFormState({ mode: "create" })}
        onDisable={(plugin) => void runPluginAction(plugin, disablePlugin)}
        onEnable={(plugin) => void runPluginAction(plugin, enablePlugin)}
        onOpen={(plugin) => setFormState({ mode: "update", pluginId: plugin.id })}
        onUninstall={(plugin) => void runPluginAction(plugin, uninstallPlugin)}
        servers={servers}
        title={t("mcpServers.servers")}
      />
      {connections.some((server) => server.source.kind === "config") ? <section className="space-y-3">
        <h3 className="text-sm font-semibold">{t("mcpServers.configServers")}</h3>
        {connections.filter((server) => server.source.kind === "config").map((server) => <div className="rounded-xl border border-border p-4" key={server.name}>
          <button className="flex w-full items-center justify-between text-sm" onClick={() => setConfigOpen(configOpen === server.name ? null : server.name)} type="button"><span>{server.displayName ?? server.name}</span><span className="text-muted">{t(`mcpServers.connectionState.${server.status.status}`)}</span></button>
          {"error" in server.status ? <p className="mt-2 text-xs text-danger">{apiErrorMessage(server.status.error)}</p> : null}
          {configOpen === server.name ? <McpCredentialsForm server={{ type: "config", name: server.name }} stdio={server.transport === "stdio"} /> : null}
        </div>)}
      </section> : null}
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
