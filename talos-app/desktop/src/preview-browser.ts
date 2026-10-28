import { createHash } from "node:crypto";
import { BrowserWindow, session, shell, type Session, type WebContents } from "electron";
import type { PreviewBrowserBounds, PreviewBrowserOperation, PreviewBrowserState } from "@talos/sdk-app-server/desktop";
import { WebsiteAccount } from "./browser-website";
import type { BrowserChromeState } from "./browser-window-contract";

type EmbeddedBrowser = { owner: WebContents; browser: WebsiteAccount; state: PreviewBrowserState };
/** Placement adapter for the existing WebsiteAccount owner. Separate realms
 * leave Computer's tabs, cookie store, downloads and input ownership intact. */
export class PreviewBrowserHost {
  private readonly views = new Map<string, EmbeddedBrowser>();
  private generation = 0;
  private readonly owners = new WeakSet<WebContents>();
  constructor(private readonly installResources: (session: Session) => void, private readonly stateChannel: string) {}
  async operate(value: unknown, owner: WebContents): Promise<PreviewBrowserState | null> {
    const operation = parseOperation(value);
    const key = `${owner.id}:${operation.id}`;
    let view = this.views.get(key);
    if (operation.kind === "close") { if (view) { this.views.delete(key); view.browser.close(); } return null; }
    if (!view && operation.kind === "mount") {
      const window = BrowserWindow.fromWebContents(owner);
      if (!window) throw new Error("The preview window is unavailable.");
      const realm = `talos-preview-${createHash("sha256").update(`${this.generation}:${key}`).digest("hex").slice(0, 32)}`;
      this.installResources(session.fromPartition(realm));
      const state: PreviewBrowserState = { id: operation.id, url: operation.url, title: "", back: false, forward: false, loading: true, error: null };
      const browser = new WebsiteAccount(realm, {
        window, partition: realm, bounds: boundsFor(owner, operation.bounds), visible: operation.visible,
        onState: state => { if (this.views.get(key)?.browser === browser) this.publish(key, state); },
        onPopup: url => {
          if (this.views.get(key)?.browser !== browser) return;
          if (new URL(url).protocol === "talos-preview:") void browser.operate({ kind: "navigate", url }).catch(error => console.error("Preview navigation failed", error));
          else void shell.openExternal(url).catch(error => console.error("Preview external opening failed", error));
        },
      });
      view = { owner, browser, state }; this.views.set(key, view);
      if (!this.owners.has(owner)) {
        this.owners.add(owner);
        owner.once("destroyed", () => { for (const [id, view] of this.views) if (view.owner === owner) { this.views.delete(id); view.browser.close(); } });
      }
      try { await browser.open(operation.url, () => {
        if (this.views.get(key)?.browser !== browser) throw new Error("The browser preview was closed.");
      }); }
      catch (error) { if (this.views.get(key)?.browser === browser) console.error("Browser preview load failed", error); }
    } else if (!view) throw new Error("The browser preview is unavailable.");
    if (this.views.get(key) !== view) return null;
    if (operation.kind === "layout" || operation.kind === "mount") {
      view.browser.layout(boundsFor(owner, operation.bounds), operation.visible);
      if (operation.kind === "mount" && operation.url !== view.state.url) await view.browser.operate({ kind: "navigate", url: operation.url });
    } else if (operation.kind === "navigate") await view.browser.operate({ kind: "navigate", url: operation.url });
    else await view.browser.operate({ kind: operation.kind });
    return view.state;
  }
  clear(): void { this.generation++; for (const view of this.views.values()) view.browser.close(); this.views.clear(); }
  private publish(key: string, state: BrowserChromeState): void {
    const view = this.views.get(key);
    if (!view || view.owner.isDestroyed()) return;
    view.state = { id: view.state.id, url: state.address, title: state.tabs.find(tab => tab.active)?.title ?? "", back: state.back, forward: state.forward, loading: state.loading, error: state.error };
    view.owner.send(this.stateChannel, view.state);
  }
}
function boundsFor(owner: WebContents, bounds: PreviewBrowserBounds): Electron.Rectangle {
  const zoom = owner.getZoomFactor();
  return { x: Math.round(bounds.x * zoom), y: Math.round(bounds.y * zoom), width: Math.max(0, Math.round(bounds.width * zoom)), height: Math.max(0, Math.round(bounds.height * zoom)) };
}
function parseOperation(value: unknown): PreviewBrowserOperation {
  if (!value || typeof value !== "object" || !("kind" in value) || !("id" in value) || typeof value.id !== "string") throw new Error("Invalid preview browser operation.");
  const v = value as Record<string, unknown>;
  if (["close", "back", "forward", "reload"].includes(String(v.kind))) return { kind: v.kind, id: v.id } as PreviewBrowserOperation;
  if (v.kind === "navigate" && typeof v.url === "string") return { kind: "navigate", id: value.id, url: v.url };
  if ((v.kind === "mount" || v.kind === "layout") && v.bounds && typeof v.bounds === "object" && typeof v.visible === "boolean") {
    const b = v.bounds as Record<string, unknown>;
    if ([b.x, b.y, b.width, b.height].every(coordinate => typeof coordinate === "number" && Number.isFinite(coordinate))) {
      const bounds = { x: b.x, y: b.y, width: b.width, height: b.height } as PreviewBrowserBounds;
      if (v.kind === "layout") return { kind: "layout", id: value.id, bounds, visible: v.visible };
      if (typeof v.url === "string") return { kind: "mount", id: value.id, url: v.url, bounds, visible: v.visible };
    }
  }
  throw new Error("Invalid preview browser operation.");
}
