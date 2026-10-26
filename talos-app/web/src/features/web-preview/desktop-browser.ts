import { useEffect, useState } from "react";
import { unwrapTalosApiResponse } from "@talos/sdk-core";
import type { DesktopBrowserRequest } from "@talos/sdk-app-server/desktop";
import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";
import { connectionScope } from "@/shared/store/connection-scope";

export async function desktopBrowserRequest(request: Exclude<DesktopBrowserRequest, { kind: "bind" }>) {
  const browser = getDesktopBridge()?.browserWorkspace;
  if (!browser) throw new Error("Desktop browser is unavailable.");
  return unwrapTalosApiResponse(await browser.request(request));
}

/** Both workspace Plus entries use the same runtime-specific creation path. */
export function useWorkspaceWebpageCreation() {
  const sessionId = useAppStore(state => state.desktopBrowser?.sessionId);
  const createPreview = useAppStore(state => state.createWorkspaceWebpage);
  const native = Boolean(getDesktopBridge()?.browserWorkspace);
  const [error, setError] = useState(false);
  return { disabled: native && !sessionId, error, create: () => {
    if (!native) createPreview();
    else if (sessionId) { setError(false);void desktopBrowserRequest({ kind: "new", sessionId }).catch(() => setError(true)); }
  } };
}

/** Subscribe before binding so target creation during authentication is not lost. */
export function useDesktopBrowserWorkspace() {
  const browser = getDesktopBridge()?.browserWorkspace;
  const scope = useAppStore(state => connectionScope(state.connection));
  const ready = useAppStore(state => ["authenticated", "open"].includes(state.connection.authStatus));
  useEffect(() => {
    if (!browser || !ready) return;
    let stopped = false, sessionId: string | undefined;
    const receive = (state: import("@talos/sdk-app-server/desktop").DesktopBrowserState) => {
      if (stopped) return;
      useAppStore.getState().syncDesktopBrowser(state);
      sessionId = useAppStore.getState().desktopBrowser?.sessionId;
    };
    const unsubscribe = browser.onState(receive);
    const connection = useAppStore.getState().connection;
    const token = connection.authStatus === "open" ? "" : connection.authToken;
    void browser.request({ kind: "bind", actorCredentials: token ? { credential: { type: "existing_session_token", token } } : null }).then(response => {
      const state = unwrapTalosApiResponse<import("@talos/sdk-app-server/desktop").DesktopBrowserState | null>(response);
      if (!state) return;
      if (stopped) void browser.request({ kind: "unbind", sessionId: state.sessionId });
      else receive(state);
    }).catch(error => { if (!stopped) console.error("Browser workspace binding failed", error); });
    return () => {
      stopped = true;unsubscribe();useAppStore.getState().syncDesktopBrowser(null);
      if (sessionId) void browser.request({ kind: "unbind", sessionId });
    };
  }, [browser, scope, ready]);
}
