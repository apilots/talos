import { EventEmitter } from "node:events";
import { expect, it, vi } from "vitest";
import type { BaseWindow, WebContents, WebContentsView } from "electron";
import type { WebsiteAccount } from "./browser-website";
import { BrowserWorkspace } from "./browser-workspace";

function fixture() {
  const tab = { id: "1", title: "Owned page", url: "https://example.test/", back: false, forward: false, loading: false };
  const account = { snapshot: () => [tab], attach: vi.fn(), present: vi.fn(), error: null, open: vi.fn(async() => tab), navigate: vi.fn(), remove: vi.fn() };
  const other = { ...account, snapshot: () => [{ ...tab, id: "2" }], attach: vi.fn() };
  const contents = Object.assign(new EventEmitter(), { send: vi.fn(), isDestroyed: () => false, getZoomFactor: () => 1, getURL: vi.fn(() => "file:///product.html") });
  const window = Object.assign(new EventEmitter(), { isDestroyed: () => false, getContentSize: () => [1000, 700] });
  const accounts = new Map([["alice", account], ["bob", other]]);
  const workspace = new BrowserWorkspace(user => accounts.get(user) as unknown as WebsiteAccount);
  workspace.setWindow(window as unknown as BaseWindow,{webContents:contents} as unknown as WebContentsView);
  const state = workspace.bind("alice");
  return { workspace, state, account, other, contents, window, accounts };
}
it("binds presentation to the exact product renderer and isolates account state and stale actions", async() => {
  const { workspace, state, account, contents } = fixture();
  expect(workspace.accepts(contents as unknown as WebContents)).toBe(true);
  contents.getURL.mockReturnValue("https://untrusted.test/");expect(workspace.accepts(contents as unknown as WebContents)).toBe(false);
  contents.getURL.mockReturnValue("file:///product.html#/files");expect(workspace.accepts(contents as unknown as WebContents)).toBe(true);
  expect(workspace.accepts({ isDestroyed: () => false } as WebContents)).toBe(false);
  workspace.publish("bob");expect(contents.send).not.toHaveBeenCalled();
  const next = workspace.bind("bob");expect(next.tabs.map(tab => tab.id)).toEqual(["2"]);
  expect(account.present).toHaveBeenLastCalledWith(null, null);
  await expect(workspace.request({ kind: "navigate", sessionId: state.sessionId, tabId: "1", url: "https://example.test/" })).rejects.toThrow("changed");
  await expect(workspace.request({ kind: "unbind", sessionId: state.sessionId })).rejects.toThrow("changed");
  workspace.publish("bob");expect(contents.send).toHaveBeenLastCalledWith("talos:browser-workspace-state", expect.objectContaining({ sessionId: next.sessionId }));
});
it("clamps native geometry and withdraws presentation without disposing the target", async() => {
  const { workspace, state, account } = fixture();
  await workspace.request({ kind: "present", sessionId: state.sessionId, tabId: "1", bounds: { x: 800, y: -10, width: 600, height: 900 } });
  expect(account.present).toHaveBeenLastCalledWith("1", { x: 800, y: 0, width: 200, height: 700 });
  await workspace.request({ kind: "present", sessionId: state.sessionId, tabId: null, bounds: null });
  expect(account.present).toHaveBeenLastCalledWith(null, null);expect(account.remove).not.toHaveBeenCalled();
});
it("publishes background target changes silently and reveals only explicitly created tabs", async() => {
  const { workspace, state, account, contents } = fixture();
  workspace.publish("alice");expect(contents.send).toHaveBeenLastCalledWith("talos:browser-workspace-state", expect.objectContaining({ revealTabId: null }));
  await workspace.request({ kind: "new", sessionId: state.sessionId });
  expect(account.open).toHaveBeenCalledWith("about:blank", expect.any(Function));
  expect(contents.send).toHaveBeenLastCalledWith("talos:browser-workspace-state", expect.objectContaining({ revealTabId: "1" }));
  await workspace.request({ kind: "close", sessionId: state.sessionId, tabId: "1" });expect(account.remove).toHaveBeenCalledWith("1");
});
it("invalidates the presentation token on reset and withdraws on navigation and renderer failure", async() => {
  const { workspace, state, contents, accounts } = fixture();
  accounts.clear();workspace.reset();
  const changed = contents.send.mock.calls.at(-1)![1];expect(changed.sessionId).not.toBe(state.sessionId);expect(changed.tabs).toEqual([]);
  await expect(workspace.request({ kind: "new", sessionId: state.sessionId })).rejects.toThrow("changed");
  contents.emit("did-start-navigation", { isMainFrame: true, isSameDocument: false });
  await expect(workspace.request({ kind: "new", sessionId: changed.sessionId })).rejects.toThrow("changed");
  const bob = workspace.bind("bob");contents.emit("render-process-gone");
  await expect(workspace.request({ kind: "new", sessionId: bob.sessionId })).rejects.toThrow("changed");
});
it("accepts a canonical origin renderer URL while preserving the document boundary", () => {
  const { workspace, window, contents } = fixture();
  workspace.setWindow(window as unknown as BaseWindow, { webContents: contents } as unknown as WebContentsView, "http://127.0.0.1:5173");
  contents.getURL.mockReturnValue("http://127.0.0.1:5173/#/files");
  expect(workspace.accepts(contents as unknown as WebContents)).toBe(true);
  contents.getURL.mockReturnValue("http://127.0.0.1:5173/another-document");
  expect(workspace.accepts(contents as unknown as WebContents)).toBe(false);
});
