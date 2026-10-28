import { EventEmitter } from "node:events";
import { beforeEach, expect, it, vi } from "vitest";
import type { WebContents } from "electron";
import type { EmbeddedWebsitePlacement } from "./browser-website";
const fixture = vi.hoisted(() => ({ views: [] as { realm: string; options: EmbeddedWebsitePlacement; close: ReturnType<typeof vi.fn> }[], finish: undefined as (() => void) | undefined, pending: false }));
vi.mock("electron", () => ({ BrowserWindow: { fromWebContents: () => ({}) }, session: { fromPartition: (realm: string) => ({ realm }) }, shell: { openExternal: vi.fn() } }));
vi.mock("./browser-website", () => ({ WebsiteAccount: class {
  close = vi.fn(); layout = vi.fn(); operate = vi.fn();
  constructor(realm: string, options: EmbeddedWebsitePlacement) { fixture.views.push({ realm, options, close: this.close }); }
  async open(_url: string, check: () => void) { if (fixture.pending) await new Promise<void>(resolve => { fixture.finish = resolve; }); check(); }
} }));
import { PreviewBrowserHost } from "./preview-browser";
beforeEach(() => { fixture.views = []; fixture.finish = undefined; fixture.pending = false; });
it("retires in-flight loads and isolates new-account realms and late state events", async () => {
  const install = vi.fn(); const host = new PreviewBrowserHost(install, "state");
  const owner = Object.assign(new EventEmitter(), { id: 7, getZoomFactor: () => 1, isDestroyed: () => false, send: vi.fn() }) as unknown as WebContents;
  const request = { kind: "mount", id: "resource", url: "https://example.test/", bounds: { x: 10, y: 20, width: 400, height: 600 }, visible: true };
  fixture.pending = true; const loading = host.operate(request, owner);
  host.clear(); fixture.finish!(); await loading;
  expect(fixture.views[0].close).toHaveBeenCalledOnce();
  fixture.pending = false; await host.operate(request, owner);
  expect(fixture.views[1].realm).not.toBe(fixture.views[0].realm);
  expect(install).toHaveBeenCalledTimes(2);
  fixture.views[0].options.onState({ tabs: [], address: "https://old.test/", back: false, forward: false, loading: false, error: null });
  expect(owner.send).not.toHaveBeenCalled();
  owner.emit("destroyed"); expect(fixture.views[1].close).toHaveBeenCalledOnce();
});
