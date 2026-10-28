import { EventEmitter } from "node:events";
import { beforeEach, expect, it, vi } from "vitest";
import type { WebContents } from "electron";
import type { WebsitePreviewOptions } from "./browser-website";
const fixture = vi.hoisted(() => ({ views: [] as { realm: string; options: WebsitePreviewOptions; close: ReturnType<typeof vi.fn>; changed: () => void }[], finish: undefined as (() => void) | undefined, pending: false, products: new Set<unknown>() }));
vi.mock("./desktop-window", () => ({ desktopWindows: fixture.products }));
vi.mock("electron", () => ({ BrowserWindow: { fromWebContents: () => ({}) }, session: { fromPartition: (realm: string) => ({ realm }) }, shell: { openExternal: vi.fn() } }));
vi.mock("./browser-website", () => ({ WebsiteAccount: class {
  close = vi.fn(); attach = vi.fn(); present = vi.fn(); navigate = vi.fn();
  snapshot = () => [{ id: "target", url: "https://example.test/", title: "Page", back: false, forward: false, loading: false }];
  constructor(realm: string, changed: () => void, _platform: string, options: WebsitePreviewOptions) { fixture.views.push({ realm, options, changed, close: this.close }); }
  async open(_url: string, check: () => void) { if (fixture.pending) await new Promise<void>(resolve => { fixture.finish = resolve; }); check(); }
} }));
import { PreviewBrowserHost } from "./preview-browser";
beforeEach(() => { fixture.views = []; fixture.finish = undefined; fixture.pending = false; fixture.products.clear(); });
it("retires in-flight loads and isolates new-account realms and late state events", async () => {
  const install = vi.fn(); const host = new PreviewBrowserHost(install, "state");
  const owner = Object.assign(new EventEmitter(), { id: 7, getZoomFactor: () => 1, isDestroyed: () => false, send: vi.fn() }) as unknown as WebContents;
  fixture.products.add({ window: { getContentSize: () => [1280, 820] }, view: { webContents: owner } });
  const request = { kind: "mount", id: "resource", url: "https://example.test/", bounds: { x: 10, y: 20, width: 400, height: 600 }, visible: true };
  fixture.pending = true; const loading = host.operate(request, owner);
  host.clear(); fixture.finish!(); await loading;
  expect(fixture.views[0].close).toHaveBeenCalledOnce();
  fixture.pending = false; await host.operate(request, owner);
  expect(fixture.views[1].realm).not.toBe(fixture.views[0].realm);
  expect(install).toHaveBeenCalledTimes(2);
  fixture.views[0].changed();
  expect(owner.send).not.toHaveBeenCalled();
  owner.emit("destroyed"); expect(fixture.views[1].close).toHaveBeenCalledOnce();
});
