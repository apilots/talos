import { useEffect, useRef, useState } from "react";
import { Hand, Loader2, Monitor, Pause, Play, Settings2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { BrowserSnapshot, DesktopViewGrant } from "@talos/sdk-core";
import type RFB from "@novnc/novnc/lib/rfb";
import { getTalosClient } from "@/shared/api/client";
import { userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
import {Badge} from "@/shared/ui/Badge";
import {cn} from "@/shared/lib/cn";
import { Button } from "@/shared/ui/Button";
import {useProductScope, useComputers} from "./hooks";
import {isDesktopRuntime} from "@/shared/api/desktop-client";
import {ComputerBrowserConfiguration} from "./ComputerBrowserConfiguration";
import { useAppStore } from "@/shared/store/app-store";

import { useBrowser } from "./browser-query";

/** Capability-gated entry, reused by the existing session header and Computer inspector. */
export function ComputerDesktopButton({ nodeId, online, remoteOnly = false, label, className }: { nodeId?: string; online: boolean; remoteOnly?: boolean; label?: string; className?: string }) {
  const { t } = useTranslation("computers");
  const open = useAppStore((state) => state.openWorkspaceWindow);
  const browser = useBrowser(nodeId, online);
  const snapshot=browser.data;
  if (remoteOnly && (snapshot?.host !== "docker" || snapshot.display.kind !== "dockerDesktop")) return null;
  if (!nodeId || !snapshot?.enabled || (snapshot.display.kind !== "dockerDesktop" && !snapshot.targets.length)) return null;
  return <Button variant="ghost" className={cn("shrink-0 gap-2 whitespace-nowrap px-2 py-2 text-xs sm:px-3.5",className)} aria-label={label ?? t(snapshot.display.kind === "dockerDesktop" ? "desktop.open" : "desktop.browser")} title={label ?? t(snapshot.display.kind === "dockerDesktop" ? "desktop.open" : "desktop.browser")} onClick={(event) => { event.currentTarget.focus(); open({ kind: "computer", nodeId }); }}>
    <Monitor aria-hidden="true" size={16} /><span className={label ? undefined : "hidden sm:inline"}>{label ?? t(snapshot.display.kind === "dockerDesktop" ? "desktop.open" : "desktop.browser")}</span>
  </Button>;
}

/** Business content only. AppLayout owns windows, geometry and focus return. */
export function ComputerDesktop({ nodeId, expanded, onExitInput }: { nodeId: string; expanded: boolean; onExitInput: () => void }) {
  const { t } = useTranslation("computers");
  const scope = useProductScope();
  const query = useBrowser(nodeId);
  const browser = query.data;
  const local=useComputers();
  const [configurationOpen,setConfigurationOpen]=useState(false);
  const remote=browser?.display.kind==="dockerDesktop";
  const selected=browser?.targets.find(target=>target.targetId===browser.selectedTargetId);
  const canOpenLocal=isDesktopRuntime() && local.localId===nodeId && selected?.localOpen==="desktopOwned";
  const [viewerId] = useState(() => crypto.randomUUID());
  const [grant, setGrant] = useState<DesktopViewGrant | null>(null);
  const [connected, setConnected] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [streamFailure, setStreamFailure] = useState<"disconnected" | "controlDisconnected" | null>(null);
  const screen = useRef<HTMLDivElement>(null);
  const client = useRef<RFB | null>(null);
  const initialGeneration = useRef<string | null>(null);
  const switching = useRef(false);
  const requests = useRef(0);
  const current = useRef({scope, nodeId, generation: browser?.control.generation, display:browser?.display.kind, available: false});
  current.current = {scope, nodeId, generation: browser?.control.generation, display:browser?.display.kind,
    available: Boolean(browser?.enabled)};
  useEffect(() => () => { requests.current++; }, []);
  const ownController = grant?.inputMode === "control" && browser?.control.owner.kind === "human"
    && browser.control.owner.controllerId === grant.controllerId && browser.control.epoch === grant.inputEpoch;

  function isCurrent(request: number, snapshot: BrowserSnapshot) {
    const latest = current.current;
    return request === requests.current && latest.scope === scope && latest.nodeId === nodeId
      && latest.available && latest.generation === snapshot.control.generation && latest.display===snapshot.display.kind;
  }
  async function open(snapshot: BrowserSnapshot, control = false) {
    const request = ++requests.current;
    if (!isCurrent(request, snapshot) || snapshot.display.kind!=="dockerDesktop") return;
    switching.current = true;
    setConnected(false); setError(null); setStreamFailure(null);
    const owner = snapshot.control.owner;
    try {
      if (control && owner.kind !== "handingOver") throw new Error(t("desktop.changed"));
      const value = await getTalosClient(nodeId).desktop.open({
        nodeId, targetId: "managed", viewerId, generation: snapshot.control.generation,
        inputMode: control ? "control" : "readOnly",
        controllerId: control && owner.kind === "handingOver" ? owner.controllerId : null,
        inputEpoch: control ? snapshot.control.epoch : null,
      });
      if (isCurrent(request, snapshot)) setGrant(value);
    } catch (failure) {
      if (isCurrent(request, snapshot)) { switching.current = false; setError(failure); }
    }
  }
  useEffect(() => {
    requests.current++;
    if (!browser?.enabled || browser.display.kind !== "dockerDesktop") {
      initialGeneration.current = null; setGrant(null); setBusy(false); return;
    }
    if (initialGeneration.current === browser.control.generation) return;
    const snapshot = browser;
    // Effect replay must not consume a second grant or viewer slot.
    const task = setTimeout(() => {
      initialGeneration.current = snapshot.control.generation;
      void open(snapshot);
    }, 0);
    return () => { requests.current++; clearTimeout(task); };
  }, [browser?.control.generation, browser?.enabled, browser?.display.kind]);

  useEffect(() => {
    if (!grant || !screen.current) return;
    let active = true;
    let socket: WebSocket | null = null;
    let rfb: RFB | null = null;
    let ended = false;
    void import("@novnc/novnc/lib/rfb").then(({ default: RFBClient }) => {
      if (!active || !screen.current) return;
      socket = getTalosClient(nodeId).desktop.connect(grant);
      socket.binaryType = "arraybuffer";
      socket.onopen = () => {
        if (!active || !socket || !screen.current) return;
        rfb = new RFBClient(screen.current, socket, { shared: true });
        rfb.background = "transparent";
        rfb.scaleViewport = true;
        rfb.resizeSession = false;
        rfb.viewOnly = true;
        rfb.qualityLevel = 6;
        rfb.compressionLevel = 2;
        client.current = rfb;
        rfb.addEventListener("connect", () => { if (active) { switching.current = false; setConnected(true); setError(null); setStreamFailure(null); void query.refetch(); } });
        rfb.addEventListener("disconnect", () => {
          ended = true;
          if (active) {
            setConnected(false);
            if (!switching.current) setStreamFailure(grant.inputMode === "control" ? "controlDisconnected" : "disconnected");
            void query.refetch();
          }
        });
      };
      socket.onerror = (event) => {
        if (active) {
          if (event instanceof ErrorEvent && event.error) setError(event.error);
          setStreamFailure(grant.inputMode === "control" ? "controlDisconnected" : "disconnected");
        }
      };
    }).catch((failure) => { if (active) setError(failure); });
    return () => { active = false; client.current = null; if (!ended) rfb?.disconnect(); socket?.close(); };
  }, [grant, nodeId]);
  useEffect(() => {
    if (client.current) {
      client.current.viewOnly = !connected || !ownController;
      if (connected && ownController) client.current.focus();
    }
  }, [connected, ownController]);

  async function operate(action: "take" | "release" | "resume" | "pause" | "openLocal") {
    if (!browser) return;
    const request = ++requests.current;
    setBusy(true); setError(null);
    switching.current = true;
    try {
      const expectedRevision = browser.control.revision;
      const operation = action === "openLocal" ? {kind:"openLocal" as const} : action === "take" ? { kind: "takeControl" as const, expectedRevision }
        : action === "resume" || action === "pause" ? { kind: action, expectedRevision }
        : { kind: "releaseControl" as const, controllerId: grant?.controllerId ?? "", expectedRevision };
      const snapshot = await getTalosClient(nodeId).browser.operate({ operation });
      if (!isCurrent(request, browser)) return;
      if (action === "pause" || action === "openLocal" || !remote) switching.current = false;
      else await open(snapshot, action === "take");
      await query.refetch();
    } catch (failure) { if (isCurrent(request, browser)) { switching.current = false; setError(failure); await query.refetch(); } }
    finally { if (current.current.generation === browser.control.generation) setBusy(false); }
  }
  const owner = browser?.control.owner.kind;
  const available = browser?.enabled && (remote || selected?.readiness.status === "ready");
  const status = !remote ? !browser?.enabled ? "browserDisabled" : selected?.readiness.status === "needsConnection" ? "needsConnection" : !available ? "browserUnavailable" : owner === "suspended" ? "suspended" : "idle" : !available ? "unavailable" : !connected ? error || streamFailure ? "disconnected" : "connecting" : ownController ? "ownHuman" : owner ?? "idle";
  const stateIndicator = <span role="status" className="inline-flex items-center gap-2 text-xs font-medium text-ink">
    <span aria-hidden="true" className={cn("size-1.5 shrink-0 rounded-full", !connected && remote ? "bg-muted/50" : ownController || owner === "suspended" ? "bg-warning" : "bg-success")} />
    {t(`desktop.state.${status}`)}
  </span>;
  const actions = <div role="group" aria-label={t("desktop.controls")} className="flex flex-wrap items-center justify-center gap-2 [&_button]:h-9 [&_button]:whitespace-nowrap [&_button]:px-4 [&_button]:py-2 [&_button]:text-xs">
    {remote && !connected && (error || streamFailure) && browser && available ? <Button variant="secondary" disabled={busy} onClick={() => void open(browser)}>{t("desktop.reconnect")}</Button> : null}
    {ownController ? <Button disabled={busy} onClick={() => void operate("release")}><Play aria-hidden="true" size={13} />{t("desktop.release")}</Button> : <>
      {(connected || !remote) && (owner === "idle" || owner === "agent" || owner === "suspended") ? <Button variant="ghost" disabled={busy || !available} onClick={() => void operate(owner === "suspended" ? "resume" : "pause")}>{owner === "suspended" ? <Play aria-hidden="true" size={13} /> : <Pause aria-hidden="true" size={13} />}{t(owner === "suspended" ? "desktop.resume" : "desktop.pause")}</Button> : null}
      {remote ? <Button disabled={busy || !available || !connected || owner === "human" || owner === "handingOver"} onClick={() => void operate("take")}>{busy ? <Loader2 aria-hidden="true" size={13} className="animate-spin motion-reduce:animate-none" /> : <Hand aria-hidden="true" size={14} />}{t("desktop.take")}</Button> : canOpenLocal ? <Button disabled={busy || !available} onClick={() => void operate("openLocal")}><Monitor aria-hidden="true" size={14} />{t("desktop.openLocal")}</Button> : null}
    </>}
  </div>;
  const failure = error || query.error || streamFailure ? <p role="alert" className="rounded-lg border border-danger/15 bg-danger/5 px-3 py-2 text-xs leading-5 text-danger">{error || query.error ? userVisibleErrorMessage(error ?? query.error, t("desktop.unavailable")) : t(`desktop.${streamFailure}`)}</p> : null;
  return <section aria-label={t("desktop.browser")} className="flex h-full min-h-0 flex-col overflow-hidden bg-background">
    <header className="flex h-11 shrink-0 items-center gap-2 border-b border-border/60 px-4">
      <h2 className="min-w-0 flex-1 truncate text-xs font-medium text-muted">{t("desktop.configuration.title")}</h2>
      {connected ? <Badge tone={ownController ? "warning" : "default"} className="shrink-0 px-2 py-0.5 normal-case tracking-normal">{t(ownController ? "desktop.controlling" : "desktop.readOnly")}</Badge> : null}
      {remote ? <Button variant="ghost" className="p-2" aria-label={t("desktop.configure")} aria-expanded={configurationOpen} title={t("desktop.configure")} onClick={() => setConfigurationOpen(value => !value)}><Settings2 aria-hidden="true" size={15} /></Button> : null}
    </header>
    {remote ? <div className="flex min-h-0 flex-1 flex-col overflow-y-auto bg-surface/25 p-4 sm:p-5">
      <div className="my-auto w-full shrink-0 self-center" style={expanded ? { maxWidth: "min(1440px, calc((100dvh - 312px) * 1.6))" } : undefined}>
        <div className="relative aspect-[16/10] w-full overflow-hidden rounded-xl border border-border/70 bg-surface shadow-[0_8px_28px_-16px_rgb(0_0_0_/_0.22)]">
          <div ref={screen} role="group" tabIndex={connected ? 0 : -1}
            onFocus={(event) => { if (event.target === event.currentTarget && ownController) client.current?.focus(); }}
            onKeyDownCapture={(event) => {
              if (event.ctrlKey && event.altKey && event.key === "Escape") {
                event.preventDefault(); event.stopPropagation(); onExitInput();
              }
            }}
            className="h-full w-full overflow-hidden rounded-xl focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-brand"
            aria-label={t("desktop.screen")} aria-description={t("desktop.keyboardHelp")} />
          {!connected ? <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-3 bg-background/95 px-6 text-center">
            <span className="flex size-11 items-center justify-center rounded-2xl border border-border/70 bg-surface text-muted">{available && !error && !streamFailure ? <Loader2 aria-hidden="true" size={19} className="animate-spin motion-reduce:animate-none" /> : <Monitor aria-hidden="true" size={20} />}</span>
            <p className="text-sm font-medium text-ink">{t(!available ? "desktop.unavailable" : error || streamFailure ? "desktop.offlineView" : "desktop.loadingView")}</p>
          </div> : null}
          {configurationOpen ? <section aria-label={t("desktop.configure")} className="absolute inset-x-0 top-0 z-10 max-h-full overflow-y-auto border-b border-border bg-background p-5 shadow-panel"><ComputerBrowserConfiguration nodeId={nodeId} /></section> : null}
        </div>
        <footer className="flex flex-col items-center gap-3 px-2 pb-1 pt-5 text-center">
          {stateIndicator}
          {actions}
          <p className="max-w-md text-[11px] leading-5 text-muted">{t(ownController ? "desktop.keyboardHelp" : "desktop.viewHint")}</p>
        </footer>
        {failure ? <div className="mt-3">{failure}</div> : null}
      </div>
    </div> : <>
      <div className="min-h-0 flex-1 overflow-y-auto p-5"><ComputerBrowserConfiguration nodeId={nodeId} /></div>
      {failure ? <div className="mx-4 mb-3">{failure}</div> : null}
      <footer className="flex flex-wrap items-center justify-between gap-3 border-t border-border/70 px-5 py-4">
        <div className="min-w-0 flex-1 space-y-1">{stateIndicator}<p className="text-[11px] leading-5 text-muted">{t("desktop.localHint")}</p></div>
        {actions}
      </footer>
    </>}
  </section>;
}
