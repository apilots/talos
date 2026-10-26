import { randomUUID } from "node:crypto";
import type { BaseWindow, WebContents, WebContentsView } from "electron";
import type { DesktopBrowserRequest, DesktopBrowserState, DesktopBrowserBounds } from "@talos/sdk-app-server/desktop";
import type { WebsiteAccount } from "./browser-website";
import { TALOS_IPC } from "./ipc";

/** Presentation of existing account targets; never owns another target registry. */
export class BrowserWorkspace {
  private window: BaseWindow | undefined;
  private product: WebContents | undefined;
  private binding: { user: string; sessionId: string } | undefined;
  private revision = 0;
  private documentUrl = "";
  contextVersion = 0;
  constructor(private readonly account: (user: string, create?: boolean) => WebsiteAccount | undefined) {}

  setWindow(window: BaseWindow, view: WebContentsView, documentUrl = view.webContents.getURL()): void {
    this.unbind();
    this.window = window;
    this.product = view.webContents;
    this.documentUrl = documentUrl.split("#")[0];
    view.webContents.on("did-start-navigation", event => {
      if (this.window === window && event.isMainFrame && !event.isSameDocument) this.unbind();
    });
    view.webContents.on("render-process-gone", () => { if (this.window === window) this.unbind(); });
    window.once("closed", () => { if (this.window === window) { const user=this.binding?.user;this.unbind();if(user)this.account(user)?.attach(undefined);this.window = undefined;this.product=undefined; } });
  }
  accepts(contents: WebContents): boolean { return this.product === contents && !contents.isDestroyed() && contents.getURL().split("#")[0] === this.documentUrl; }
  bind(user: string): DesktopBrowserState {
    this.unbind();
    this.binding = { user, sessionId: randomUUID() };
    this.account(user)?.attach(this.window);
    return this.state();
  }
  unbind(): void {
    this.contextVersion++;
    if (this.binding) this.account(this.binding.user)?.present(null, null);
    this.binding = undefined;
  }
  reset(): void {
    if (this.binding) { this.binding.sessionId = randomUUID(); this.publish(this.binding.user); }
  }
  private state(revealTabId: string | null = null): DesktopBrowserState {
    if (!this.binding) throw new Error("Browser workspace is not bound.");
    const account = this.account(this.binding.user);
    return { sessionId: this.binding.sessionId, revision: ++this.revision, tabs: account?.snapshot() ?? [], revealTabId, error: account?.error ?? null };
  }
  publish(user: string, revealTabId: string | null = null): void {
    if (this.binding?.user !== user || !this.window || this.window.isDestroyed()) return;
    this.account(user)?.attach(this.window);
    this.product!.send(TALOS_IPC.browserState, this.state(revealTabId));
  }
  async request(request: Exclude<DesktopBrowserRequest, { kind: "bind" }>): Promise<DesktopBrowserState | null> {
    const binding = this.binding;
    if (!binding || request.sessionId !== binding.sessionId) throw new Error("Browser workspace changed.");
    if (request.kind === "unbind") { this.unbind(); return null; }
    const account = this.account(binding.user, true)!;
    account.attach(this.window);
    const check = () => { if (this.binding !== binding || request.sessionId !== binding.sessionId) throw new Error("Browser workspace changed."); };
    if (request.kind === "new") {
      const tab = await account.open("about:blank", check);check();this.publish(binding.user, tab.id);
    } else if (request.kind === "present") {
      account.present(request.tabId, request.bounds ? this.bounds(request.bounds) : null);
    } else if (request.kind === "close") await account.remove(request.tabId);
    else if (["navigate", "back", "forward", "reload"].includes(request.kind)) {
      await account.navigate(request.tabId, request.kind, request.kind === "navigate" ? request.url : undefined);
    } else throw new Error("Unknown browser presentation action.");
    check();return null;
  }
  private bounds(bounds: DesktopBrowserBounds): DesktopBrowserBounds | null {
    if (!this.window) return null;
    if (![bounds.x, bounds.y, bounds.width, bounds.height].every(Number.isFinite)) throw new Error("Invalid browser viewport.");
    const [width, height] = this.window.getContentSize();
    const zoom = this.product!.getZoomFactor();
    bounds = { x: bounds.x * zoom, y: bounds.y * zoom, width: bounds.width * zoom, height: bounds.height * zoom };
    const x = Math.max(0, Math.min(width, Math.round(bounds.x)));
    const y = Math.max(0, Math.min(height, Math.round(bounds.y)));
    const w = Math.min(width - x, Math.round(bounds.width));
    const h = Math.min(height - y, Math.round(bounds.height));
    return w > 0 && h > 0 ? { x, y, width: w, height: h } : null;
  }
}
