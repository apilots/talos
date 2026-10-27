import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowRight, Globe2, RotateCw } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { DesktopBrowserTab } from "@talos/sdk-app-server/desktop";
import { useAppStore } from "@/shared/store/app-store";
import { normalizeWorkspaceWebpageUrl } from "@/shared/store/workspace-window";
import { actionClasses } from "@/shared/ui/controlStyles";
import { cn } from "@/shared/lib/cn";
import { desktopBrowserRequest } from "./desktop-browser";

/** DOM geometry only. The real website remains isolated in an Electron native view. */
export function DesktopBrowserPage({ tab, selected }: { tab: DesktopBrowserTab; selected: boolean }) {
  const { t } = useTranslation("webPreview");
  const sessionId = useAppStore(state => state.desktopBrowser?.sessionId);
  const hostError = useAppStore(state => state.desktopBrowser?.error);
  const [address, setAddress] = useState(tab.url === "about:blank" ? "" : tab.url);
  const [error, setError] = useState(false);
  const editing = useRef(false);
  const viewport = useRef<HTMLDivElement>(null);
  useEffect(() => { if (!editing.current) setAddress(tab.url === "about:blank" ? "" : tab.url); }, [tab.url]);
  useEffect(() => {
    const element = viewport.current;
    if (!element || !sessionId || !selected) return;
    let last = "";
    const report = () => {
      const rect = element.getBoundingClientRect();
      const owner = element.closest("dialog");
      const overlay = [...document.querySelectorAll('dialog[open], [role="dialog"], [role="alertdialog"], [role="menu"]')].some(node => node !== owner && !node.contains(element) && !node.closest('[hidden], [inert], [aria-hidden="true"]') && node.getClientRects().length > 0);
      const visible = document.visibilityState !== "hidden" && !element.closest("[hidden], [inert]") && (!owner || owner.open) && !overlay && rect.width > 0 && rect.height > 0;
      const bounds = visible ? { x: rect.x, y: rect.y, width: rect.width, height: rect.height } : null;
      const key = JSON.stringify(bounds);if (key === last) return;last = key;
      void desktopBrowserRequest({ kind: "present", sessionId, tabId: visible ? tab.id : null, bounds }).catch(() => setError(true));
    };
    const resize = new ResizeObserver(report);resize.observe(element);
    const mutations = new MutationObserver(report);
    mutations.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open", "hidden", "inert", "aria-hidden", "class", "style"] });
    window.addEventListener("resize", report);document.addEventListener("visibilitychange", report);document.addEventListener("scroll", report, true);
    report();
    return () => {
      resize.disconnect();mutations.disconnect();window.removeEventListener("resize", report);document.removeEventListener("visibilitychange", report);document.removeEventListener("scroll", report, true);
      void desktopBrowserRequest({ kind: "present", sessionId, tabId: null, bounds: null }).catch(() => {});
    };
  }, [selected, sessionId, tab.id]);
  const act = (kind: "back" | "forward" | "reload" | "navigate", url?: string) => {
    if (!sessionId) return;setError(false);
    const request = kind === "navigate" ? { kind, sessionId, tabId: tab.id, url: url! } : { kind, sessionId, tabId: tab.id };
    void desktopBrowserRequest(request).catch(() => setError(true));
  };
  const button = cn("flex size-8 shrink-0 items-center justify-center rounded-lg disabled:opacity-35 focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand", actionClasses.ghost);
  return <section aria-label={t("browserTitle")} className="flex h-full min-h-0 flex-col bg-background">
    <div className="shrink-0 border-b border-divider bg-surface px-3 py-2">
      <div className="flex items-center gap-1.5">
        <button type="button" className={button} aria-label={t("back")} disabled={!tab.back} onClick={() => act("back")}><ArrowLeft size={15} /></button>
        <button type="button" className={button} aria-label={t("forward")} disabled={!tab.forward} onClick={() => act("forward")}><ArrowRight size={15} /></button>
        <button type="button" className={button} aria-label={t("refresh")} onClick={() => act("reload")}><RotateCw size={15} className={tab.loading ? "animate-spin motion-reduce:animate-none" : undefined} /></button>
        <form className="flex h-9 min-w-0 flex-1 items-center rounded-full border border-border bg-card focus-within:ring-2 focus-within:ring-brand/20" onSubmit={event => {
          event.preventDefault();const url = normalizeWorkspaceWebpageUrl(address.includes("://") ? address : `https://${address}`);
          if (!url) { setError(true);return; }editing.current = false;act("navigate", url);
        }}>
          <Globe2 size={14} className="ml-3 shrink-0 text-muted" />
          <input aria-label={t("address")} value={address} onFocus={() => { editing.current = true; }} onBlur={() => { editing.current = false; }} onChange={event => setAddress(event.target.value)} autoComplete="off" spellCheck={false} className="h-full min-w-0 flex-1 bg-transparent px-2 text-xs text-ink outline-none" />
          <button type="submit" className={button} aria-label={t("go")}><ArrowRight size={15} /></button>
        </form>
      </div>
      {error || hostError ? <p role="alert" className="mt-2 text-xs text-danger">{t("browserFailed")}</p> : null}
    </div>
    <div ref={viewport} aria-label={t("browserContent")} className="min-h-0 flex-1 bg-white" />
  </section>;
}
