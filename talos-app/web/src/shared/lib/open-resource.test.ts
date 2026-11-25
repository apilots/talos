// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { inlineCodeResourceReference, openResource, openResourceLink } from "./open-resource";
const host = vi.hoisted(() => ({ desktop: false, openExternal: vi.fn() }));
vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: () => host.desktop ? { openExternal: host.openExternal } : undefined }));
beforeEach(() => {
  host.desktop = false; host.openExternal.mockReset().mockResolvedValue(undefined);
  useAppStore.setState({ workspaceOwner: null, workspaceSnapshots: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false });
});
it("opens ordinary web links embedded on Web and deduplicates within the current context", async () => {
  await openResourceLink("https://example.test"); await openResourceLink("https://example.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({ kind: "webpage", url: "https://example.test/" });
});
it("opens ordinary web links embedded on Desktop while retaining explicit external intent", async () => {
  host.desktop = true;
  await openResourceLink("https://example.test");
  expect(host.openExternal).not.toHaveBeenCalled();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({ kind: "webpage", url: "https://example.test/" });
  await openResource({ kind: "webpage", url: "https://example.test", intent: "external" });
  expect(host.openExternal).toHaveBeenCalledWith("https://example.test/");
});
it("opens registered file references on either client without guessing arbitrary workspace paths", async () => {
  host.desktop = true;
  await openResourceLink("talos-file://file-123");
  expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "file", fileId: "file-123" }]);
  for (const url of ["file:///etc/passwd", "./report.html", "/workspace/report.html", "report.md:12", "https://user:pass@example.test", "javascript:alert(1)"]) await expect(openResourceLink(url)).rejects.toThrow();
  expect(host.openExternal).not.toHaveBeenCalled();
});
it("opens root-level and nested session file citations inside a session context", async () => {
  useAppStore.getState().setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "session-1" } }, true);
  await openResourceLink("report.md:12");
  await openResourceLink("output/report.html");
  expect(useAppStore.getState().workspaceWindows).toEqual([
    { kind: "sessionFile", sessionId: "session-1", path: "report.md", position: { kind: "text", resourcePath: null, line: 12, encoding: "utf-8" } },
    { kind: "sessionFile", sessionId: "session-1", path: "output/report.html" },
  ]);
});
it("recognizes inline code file paths and safe URLs without matching commands or code symbols", () => {
  for (const value of [
    "report.html",
    "./report.html",
    "output/self-review.html",
    "docs/design.md",
    "README.md",
    "src/lib.rs:27",
    "lib.rs:27:5",
    "src/lib.rs#L27",
    "/Users/alice/repo/report.html",
    "file:///tmp/report.html",
    "talos-file://file-123",
    "https://example.test/docs",
    "http://localhost:3000/app",
    "logs/server.log",
  ]) {
    expect(inlineCodeResourceReference(value, true)).toBe(value);
  }
  expect(inlineCodeResourceReference("report.html", false)).toBeNull();
  expect(inlineCodeResourceReference("https://example.test/docs", false)).toBe("https://example.test/docs");
  expect(inlineCodeResourceReference("talos-file://file-123", false)).toBe("talos-file://file-123");
  for (const value of [
    "console.log",
    "window.talos",
    "document.activeElement",
    "Web/Desktop",
    "prepare/read/close",
    "application/json",
    "text/html",
    "session/active_turn",
    "origin/main",
    "/status",
    "/worktree create origin/main",
    "pnpm --dir talos-app test",
    "src/*.ts",
    "v0.1.19",
    "example.com",
    "@talos/sdk-core",
    "https://user:pass@example.test",
    "javascript:alert(1)",
    "file:///",
    "file:///tmp/*.md",
    "foo#bar/baz.ts",
    "127.0.0.1:8080",
  ]) {
    expect(inlineCodeResourceReference(value, true)).toBeNull();
  }
});
it("isolates the same URL between session owners", async () => {
  const store = useAppStore.getState();
  store.setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "a" } }, true);
  await openResourceLink("https://example.test");
  const first = useAppStore.getState().workspaceActiveKey;
  store.setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "b" } }, true);
  await openResourceLink("https://example.test");
  expect(useAppStore.getState().workspaceActiveKey).not.toBe(first);
});
