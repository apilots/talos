import { useEffect, useId, useRef, useState } from "react";
import type { PreviewBrowserState } from "@talos/sdk-app-server/desktop";
import { getDesktopBridge } from "@/shared/api/desktop-client";

/** DOM placement only. The native browser owner supplies history and events;
 * Web keeps the existing sandboxed frame and advertises no fabricated history. */
export function useBrowserSurface(url: string | undefined, onNavigate?: (url: string, title: string) => void) {
  const bridge = getDesktopBridge()?.preview;
  const native = bridge?.browser ? bridge : undefined;
  const id = useId();
  const element = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<PreviewBrowserState>();
  const latest = useRef({ url, onNavigate }); latest.current = { url, onNavigate };
  const currentUrl = useRef(url);
  const mounted = useRef(false);
  const available = !!url;
  useEffect(() => {
    if (!native || !available || !element.current) return;
    let disposed = false;
    const publish = (state: PreviewBrowserState | null) => {
      if (!state || disposed || state.id !== id) return;
      currentUrl.current = state.url; setState(state);
      if (/^https?:/.test(state.url)) latest.current.onNavigate?.(state.url, state.title);
    };
    const stop = native.onBrowserState(publish);
    const run = (request: Parameters<typeof native.browser>[0]) => void native.browser(request).then(publish, error => {
      if (!disposed) { console.error("Browser preview failed", error); setState({ id, url: latest.current.url ?? "", title: "", back: false, forward: false, loading: false, error: "loadFailed" }); }
    });
    let previousPlacement = "";
    const layout = () => {
      const target = element.current;
      if (!target) return;
      const rect = target.getBoundingClientRect();
      const overlay = document.querySelector("[data-radix-popper-content-wrapper], [role='dialog'][data-state='open'], dialog[open]:not(#talos-right-workspace)");
      const visible = !!rect.width && !!rect.height && !document.hidden && !overlay && (target.checkVisibility?.() ?? true);
      const bounds = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      const placement = JSON.stringify([bounds, visible]);
      if (mounted.current && previousPlacement === placement) return;
      previousPlacement = placement;
      if (!mounted.current) {
        mounted.current = true; currentUrl.current = latest.current.url;
        run({ kind: "mount", id, url: latest.current.url!, bounds, visible });
      } else run({ kind: "layout", id, bounds, visible });
    };
    const resize = new ResizeObserver(layout); resize.observe(element.current);
    const overlays = new MutationObserver(layout); overlays.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-state", "open", "hidden", "inert"] });
    document.addEventListener("visibilitychange", layout); window.addEventListener("resize", layout);
    layout();
    return () => {
      disposed = true; mounted.current = false; stop(); resize.disconnect(); overlays.disconnect();
      document.removeEventListener("visibilitychange", layout); window.removeEventListener("resize", layout);
      void native.browser({ kind: "close", id }).catch(error => console.warn("Browser preview release failed", error));
    };
  }, [native, id, available]);
  useEffect(() => {
    if (native && mounted.current && url && url !== currentUrl.current) {
      currentUrl.current = url;
      void native.browser({ kind: "navigate", id, url }).catch(error => console.error("Browser preview navigation failed", error));
    }
  }, [native, id, url]);
  return { native: !!native, element, state,
    command: (kind: "back" | "forward" | "reload") => void native?.browser({ kind, id }).catch(error => console.error("Browser preview navigation failed", error)) };
}
export type BrowserSurface = ReturnType<typeof useBrowserSurface>;
