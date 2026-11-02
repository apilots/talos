// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { openResource, openResourceLink } from "./open-resource";
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
it("uses the actual Desktop default browser for link intent but retains explicit preview intent", async () => {
  host.desktop = true;
  await openResourceLink("https://example.test");
  expect(host.openExternal).toHaveBeenCalledWith("https://example.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  await openResource({ kind: "webpage", url: "https://example.test", intent: "preview" });
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
});
it("opens registered file references on either client without guessing arbitrary workspace paths", async () => {
  host.desktop = true;
  await openResourceLink("talos-file://file-123");
  expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "file", fileId: "file-123" }]);
  for (const url of ["file:///etc/passwd", "./report.html", "/workspace/report.html", "https://user:pass@example.test", "javascript:alert(1)"]) await expect(openResourceLink(url)).rejects.toThrow();
  expect(host.openExternal).not.toHaveBeenCalled();
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
