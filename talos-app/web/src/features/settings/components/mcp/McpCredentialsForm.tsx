import type { McpDefinitionRef, McpOAuthConfig } from "@talos/sdk-browser";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { closePreparedExternalWindow, prepareExternalAuthWindow, redirectToExternalUrl } from "@/features/auth/navigation";
import { getTalosClient } from "@/shared/api/client";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { FormSection, parseEntries, TextArea, TextInput } from "./McpFormPrimitives";

export function McpCredentialsForm({ server, stdio, onUpdated }: { server: McpDefinitionRef; stdio: boolean; onUpdated?: () => Promise<void> }) {
  const { t } = useTranslation("settings");
  const [mode, setMode] = useState<"bearer" | "header" | "oauth">("bearer");
  const initialized = useRef(false);
  const authorizing = useRef(false);
  const [scopes, setScopes] = useState("");
  const [oauth, setOAuth] = useState<McpOAuthConfig>({ scopes: [] });
  const [header, setHeader] = useState("");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ type: "entries" } | { type: "api"; cause: unknown } | null>(null);
  const auth = useQuery({ queryKey: ["talos", "mcp-auth", server], queryFn: () => getTalosClient().mcp.readAuthentication({ server }), refetchInterval: (query) => query.state.data?.state.type === "authorizing" ? 1000 : false });
  useEffect(() => {
    if (!auth.data || !auth.isFetchedAfterMount) return;
    if (!initialized.current) {
      initialized.current = true;
      if (auth.data.authentication.type === "oauth") {
        setMode("oauth"); setOAuth(auth.data.authentication.config); setScopes(auth.data.authentication.config.scopes.join(" "));
      } else if (auth.data.authentication.type === "header") {
        setMode("header"); setHeader(auth.data.authentication.name);
      }
    }
    if (auth.data.state.type === "authorizing") authorizing.current = true;
    else if (authorizing.current && auth.data.state.type === "authenticated") {
      authorizing.current = false;
      void onUpdated?.();
    }
  }, [auth.data, auth.isFetchedAfterMount, onUpdated]);
  async function authorize() {
    setError(null);
    setBusy(true);
    const prepared = prepareExternalAuthWindow();
    try {
      const result = await getTalosClient().mcp.startAuthentication({ server, config: { ...oauth, scopes: scopes.split(/\s+/).filter(Boolean) }, clientSecret: secret || undefined });
      authorizing.current = true;
      redirectToExternalUrl(result.authorizationUrl, prepared);
      setSecret("");
      const current = await auth.refetch();
      if (authorizing.current && current.data?.state.type === "authenticated") {
        authorizing.current = false;
        await onUpdated?.();
      }
    } catch (cause) { closePreparedExternalWindow(prepared); setError({ type: "api", cause }); }
    finally { setBusy(false); }
  }
  async function save(clear: boolean) {
    setError(null);
    let env: Record<string, string> | undefined;
    if (stdio && !clear) {
      try { env = parseEntries(secret); } catch { setError({ type: "entries" }); return; }
    }
    setBusy(true);
    try {
      await getTalosClient().mcp.updateCredentials({ server, change: clear ? { type: "clear" } : { type: "replace", credentials: stdio ? { authentication: { type: "none" }, env } : { authentication: mode === "bearer" ? { type: "bearer" } : { type: "header", name: header }, token: secret } } });
      setSecret("");
      await auth.refetch();
      await onUpdated?.();
    } catch (failure) { setError({ type: "api", cause: failure }); }
    finally { setBusy(false); }
  }
  const status = auth.data?.state;
  return <section className="mt-6 rounded-[1.05rem] border border-border/80 bg-card/45">
    <FormSection title={t("mcpServers.credentials")}>
      <p className="text-sm text-muted">{t("mcpServers.secretHint")}</p>
      {status ? <p role="status" className="text-sm text-muted">{t(`mcpServers.authState.${status.type}`)}</p> : null}
      {status?.type === "failed" ? <p className="text-sm text-danger">{apiErrorMessage(status.error)}</p> : null}
      {auth.error ? <p role="alert" className="text-sm text-danger">{userVisibleErrorMessage(auth.error, t("mcpServers.loadError"))}</p> : null}
      {!stdio ? <>
        <select aria-label={t("mcpServers.authentication")} className="form-control-focus rounded-lg border border-border bg-background p-2 text-sm" onChange={(event) => setMode(event.target.value as "bearer" | "header" | "oauth")} value={mode}>
          <option value="bearer">Bearer token</option><option value="header">API key</option><option value="oauth">OAuth</option>
        </select>
        {mode === "header" ? <TextInput aria-label={t("mcpServers.headerName")} onChange={(event) => setHeader(event.target.value)} placeholder="X-API-Key" value={header} /> : null}
        {mode === "oauth" ? <div className="space-y-3">
          <p className="text-sm text-muted">{t("mcpServers.oauthLocalHint")}</p>
          <TextInput aria-label={t("mcpServers.oauthClientId")} placeholder={t("mcpServers.oauthClientId")} value={oauth.clientId ?? ""} onChange={(event) => setOAuth({ ...oauth, clientId: event.target.value || undefined })} />
          <TextInput aria-label={t("mcpServers.oauthMetadataUrl")} placeholder={t("mcpServers.oauthMetadataUrl")} value={oauth.clientMetadataUrl ?? ""} onChange={(event) => setOAuth({ ...oauth, clientMetadataUrl: event.target.value || undefined })} />
          <TextInput aria-label={t("mcpServers.oauthCallbackPort")} placeholder={t("mcpServers.oauthCallbackPort")} type="number" min={1} max={65535} value={oauth.callbackPort ?? ""} onChange={(event) => setOAuth({ ...oauth, callbackPort: event.target.value ? Number(event.target.value) : undefined })} />
          <TextInput aria-label={t("mcpServers.oauthScopes")} placeholder={t("mcpServers.oauthScopes")} value={scopes} onChange={(event) => setScopes(event.target.value)} />
        </div> : null}
        <TextInput placeholder={mode === "oauth" ? t("mcpServers.oauthClientSecret") : undefined} aria-label={mode === "oauth" ? t("mcpServers.oauthClientSecret") : t("mcpServers.secretValue")} autoComplete="off" onChange={(event) => setSecret(event.target.value)} type="password" value={secret} />
      </> : <TextArea aria-label={t("mcpServers.secretEnvironment")} onChange={(event) => setSecret(event.target.value)} placeholder="API_KEY=value" value={secret} />}
      {error ? <p role="alert" className="text-sm text-danger">{error.type === "entries" ? t("mcpServers.invalidEntries") : userVisibleErrorMessage(error.cause, t("mcpServers.saveError"))}</p> : null}
      <div className="flex gap-3">
        <button className="rounded-lg bg-brand px-4 py-2 text-sm text-white disabled:opacity-50" disabled={busy || (mode !== "oauth" && (!secret || (!stdio && mode === "header" && !header)))} onClick={() => void (mode === "oauth" && !stdio ? authorize() : save(false))} type="button">{t(mode === "oauth" && !stdio ? "mcpServers.oauthAuthorize" : "mcpServers.replaceCredentials")}</button>
        <button className="rounded-lg border border-border px-4 py-2 text-sm text-danger disabled:opacity-50" disabled={busy || !status || status.type === "unconfigured" || status.type === "missing"} onClick={() => void save(true)} type="button">{t("mcpServers.clearCredentials")}</button>
      </div>
    </FormSection>
  </section>;
}
