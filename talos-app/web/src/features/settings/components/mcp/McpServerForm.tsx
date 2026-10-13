import type { Plugin } from "@talos/sdk-browser";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { getTalosClient } from "@/shared/api/client";
import { Button } from "@/shared/ui/Button";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { mcpServerDefinition, pluginEnabled, pluginTitle } from "@/features/plugins/lib/pluginDisplay";
import { FormSection, TextInput, TextArea, formatEntries, parseEntries } from "./McpFormPrimitives";
import { McpServerFormHeader } from "./McpServerFormHeader";
import { McpTransportFields, type TransportFields } from "./McpTransportFields";
import { McpTransportSelector, type McpTransportMode } from "./McpTransportSelector";
import { McpCredentialsForm } from "./McpCredentialsForm";

type McpServerFormProps = { onBack: () => void; onSaved: () => Promise<void> } & (
  | { mode: "create" }
  | { busy: boolean; mode: "update"; plugin: Plugin; onDisable: (plugin: Plugin) => void; onEnable: (plugin: Plugin) => void; onUninstall: (plugin: Plugin) => void }
);

export function McpServerForm(props: McpServerFormProps) {
  const { t } = useTranslation("settings");
  const plugin = props.mode === "update" ? props.plugin : null;
  const definition = plugin ? mcpServerDefinition(plugin) : null;
  const readOnly = !!plugin?.source_id;
  const [name, setName] = useState(definition?.name ?? "");
  const [mode, setMode] = useState<McpTransportMode>(definition?.transport.type === "streamable_http" ? "http" : "stdio");
  const stdio = definition?.transport.type === "stdio" ? definition.transport : null;
  const http = definition?.transport.type === "streamable_http" ? definition.transport : null;
  const [fields, setFields] = useState<TransportFields>({ command: stdio?.command ?? "", args: stdio?.args.join("\n") ?? "", env: formatEntries(stdio?.env), cwd: stdio?.cwd ?? "", url: http?.url ?? "", headers: formatEntries(http?.headers) });
  const [jsonMode, setJsonMode] = useState(false);
  const [json, setJson] = useState("");
  const [token, setToken] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<{ type: "entries" } | { type: "api"; cause: unknown } | null>(null);
  const busy = saving || (props.mode === "update" && props.busy);
  async function save() {
    setError(null);
    let env: Record<string, string> = {}, headers: Record<string, string> = {};
    if (!jsonMode) {
      try {
        if (mode === "stdio") env = parseEntries(fields.env);
        else headers = parseEntries(fields.headers);
      }
      catch { setError({ type: "entries" }); return; }
    }
    setSaving(true);
    try {
      if (jsonMode) {
        await getTalosClient().mcp.import({ json, name: name || undefined, enable: true, conflictPolicy: "reject" });
      } else {
        const hasToken = mode === "http" && Boolean(token);
        await getTalosClient().plugin.install({ target: { kind: "manual_mcp_server", server: {
          name, display_name: definition?.display_name ?? null, description: definition?.description ?? null,
          credentials: hasToken ? undefined : definition?.credentials,
          transport: mode === "stdio" ? { type: "stdio", command: fields.command, args: fields.args ? fields.args.split(/\r?\n/) : [], env, cwd: fields.cwd || null } : { type: "streamable_http", url: fields.url, headers },
        }, credentials: hasToken ? { authentication: { type: "bearer" }, token } : undefined }, enable: plugin ? pluginEnabled(plugin) : true, conflict_policy: plugin ? "replace" : "reject" });
      }
      setToken(""); setJson("");
      await props.onSaved();
    } catch (failure) { setError({ type: "api", cause: failure }); }
    finally { setSaving(false); }
  }
  const installId = plugin && "install_id" in plugin.status ? plugin.status.install_id : null;
  return <div className="mx-auto max-w-[760px] pb-10">
    <button className="mb-8 text-sm text-muted hover:text-ink" disabled={busy} onClick={props.onBack} type="button">&lt; {t("mcpServers.back")}</button>
    <McpServerFormHeader busy={busy} mode={props.mode} onDisable={props.mode === "update" ? props.onDisable : undefined} onEnable={props.mode === "update" ? props.onEnable : undefined} onUninstall={props.mode === "update" ? props.onUninstall : undefined} plugin={plugin ?? undefined} pluginName={plugin ? pluginTitle(plugin) : ""} />
    <p className="mb-4 text-sm text-muted">{t("mcpServers.backendHost")}</p>
    {readOnly ? <p className="mb-4 text-sm text-muted">{t("mcpServers.marketReadOnly")}</p> : null}
    {!plugin ? <label className="mb-4 flex items-center gap-2 text-sm"><input checked={jsonMode} onChange={(event) => setJsonMode(event.target.checked)} type="checkbox" />{t("mcpServers.importJson")}</label> : null}
    <form onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <fieldset className="overflow-hidden rounded-[1.05rem] border border-border/80 bg-card/45" disabled={busy}>
        <FormSection title={t("mcpServers.name")}><TextInput aria-label={t("mcpServers.name")} onChange={(event) => setName(event.target.value)} readOnly={!!plugin} required={!jsonMode} value={name} /></FormSection>
        {jsonMode ? <FormSection title={t("mcpServers.importJson")}><TextArea aria-label={t("mcpServers.importJson")} onChange={(event) => setJson(event.target.value)} required rows={12} value={json} /></FormSection> : <>
          <McpTransportSelector disabled={readOnly} mode={mode} onChange={setMode} />
          <McpTransportFields mode={mode} onChange={setFields} readOnly={readOnly} value={fields} />
          {!readOnly && mode === "http" ? <FormSection title={t("mcpServers.secretValue")}><TextInput aria-label={t("mcpServers.secretValue")} autoComplete="off" onChange={(event) => setToken(event.target.value)} type="password" value={token} /><p className="text-xs text-muted">{t("mcpServers.secretHint")}</p></FormSection> : null}
        </>}
      </fieldset>
      {error ? <p className="mt-3 text-sm text-danger" role="alert">{error.type === "entries" ? t("mcpServers.invalidEntries") : userVisibleErrorMessage(error.cause, t("mcpServers.saveError"))}</p> : null}
      {!readOnly ? <div className="mt-4 flex justify-end"><Button className="px-5 font-semibold" disabled={busy} type="submit">{t(plugin ? "mcpServers.replaceDefinition" : "mcpServers.save")}</Button></div> : null}
    </form>
    {installId ? <McpCredentialsForm onUpdated={props.onSaved} server={{ type: "plugin", installId }} stdio={definition?.transport.type === "stdio"} /> : null}
  </div>;
}
