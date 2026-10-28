// @vitest-environment jsdom
import { beforeEach, expect, it } from "vitest";
import { useAppStore } from "./app-store";
import { normalizeWorkspaceWebpageUrl, workspaceWindowKey } from "./workspace-window";
import { restoreWorkspaces, workspaceOwnerKey } from "./workspace-state";

const owner = { scope: "actor", context: { kind: "files" as const } };
const ownerKey = workspaceOwnerKey(owner);
const computer = (nodeId: string) => ({ kind: "computer" as const, nodeId });
const key = (nodeId: string) => workspaceWindowKey(computer(nodeId));
beforeEach(() => {
  useAppStore.getState().setConnection({ userId: "workspace-review", controlPlaneEndpoint: "http://localhost:8080" });
  useAppStore.setState({ workspaceOwner: null, workspaceSnapshots: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
  useAppStore.getState().setWorkspaceContext(owner, true);
});
it("deduplicates real Computers, selects each independently and keeps records when hidden", () => {
  const store = useAppStore.getState();
  store.openWorkspaceWindow(computer("a"));store.openWorkspaceWindow(computer("b"));store.openWorkspaceWindow(computer("a"));
  expect(useAppStore.getState().workspaceWindows).toEqual([computer("a"), computer("b")]);
  expect(useAppStore.getState().workspaceActiveKey).toBe(key("a"));
  store.setWorkspaceExpanded(true);store.setWorkspaceOpen(false);
  expect(useAppStore.getState()).toMatchObject({ workspaceOpen: false, workspaceExpanded: false, workspaceActiveKey: key("a") });
  expect(useAppStore.getState().workspaceWindows).toHaveLength(2);
  store.setWorkspaceOpen(true);store.selectWorkspaceWindow(key("b"));
  expect(useAppStore.getState().workspaceActiveKey).toBe(key("b"));
});
it("keeps the active window when closing a background window and chooses a neighbor on close", () => {
  const store = useAppStore.getState();
  for (const id of ["a", "b", "c"]) store.openWorkspaceWindow(computer(id));
  store.closeWorkspaceWindow(key("b"));expect(useAppStore.getState().workspaceActiveKey).toBe(key("c"));
  store.closeWorkspaceWindow(key("c"));expect(useAppStore.getState().workspaceActiveKey).toBe(key("a"));
  store.closeWorkspaceWindow(key("a"));store.setWorkspaceOpen(true);store.setWorkspaceExpanded(true);
  expect(useAppStore.getState()).toMatchObject({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
});
it("creates independent blank webpage tabs with stable identity through navigation", () => {
  const store=useAppStore.getState();store.openWorkspaceWindow({kind:"file",fileId:"owned-file"});
  store.createWorkspaceWebpage();const first=useAppStore.getState().workspaceActiveKey!;
  store.createWorkspaceWebpage();const second=useAppStore.getState().workspaceActiveKey!;
  expect(first).not.toBe(second);
  expect(useAppStore.getState().workspaceWindows.filter(window=>window.kind==="webpage")).toHaveLength(2);
  store.navigateWorkspaceWebpage(ownerKey,first,"https://example.test");store.navigateWorkspaceWebpage(ownerKey,second,"https://example.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(3);
  expect(useAppStore.getState().workspaceActiveKey).toBe(second);
  expect(useAppStore.getState().workspaceWindows.filter(window=>window.kind==="webpage").map(window=>window.url)).toEqual(["https://example.test/","https://example.test/"]);
  store.selectWorkspaceWindow(first);store.navigateWorkspaceWebpage(ownerKey,first,"http://localhost:3010/");
  expect(useAppStore.getState().workspaceActiveKey).toBe(first);
  store.closeWorkspaceWindow(first);store.navigateWorkspaceWebpage(ownerKey,first,"https://ignored.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(2);
});
it("rejects unsafe or credential-bearing navigation without changing a blank tab", () => {
  const store=useAppStore.getState();store.createWorkspaceWebpage();const key=useAppStore.getState().workspaceActiveKey!;
  for (const url of ["javascript:alert(1)", "file:///etc/passwd", "https://user:secret@example.test", "not a URL"]) {
    expect(normalizeWorkspaceWebpageUrl(url)).toBeNull();
    expect(()=>store.navigateWorkspaceWebpage(ownerKey,key,url)).toThrow();
  }
  expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({kind:"webpage",url:""});
});
it("retires every descriptor on account or endpoint changes, but not credential rotation", () => {
  const store = useAppStore.getState();store.openWorkspaceWindow(computer("a"));
  store.setConnection({ authToken: "rotation" });expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  store.setConnection({ userId: "other-account" });expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  store.openWorkspaceWindow({ kind: "file", fileId: "private" });
  store.setConnection({ controlPlaneEndpoint: "http://localhost:8081" });
  expect(useAppStore.getState()).toMatchObject({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
});

it("retains pane preferences through collapse and clears them with the account scope",()=>{
 const store=useAppStore.getState();store.setSidebarWidth(340);store.setWorkspaceWidth(700);store.openWorkspaceWindow(computer("a"));store.setWorkspaceOpen(false);
 expect(useAppStore.getState()).toMatchObject({sidebarWidth:340,workspaceWidth:700});
 store.setResizingPane("sidebar");store.setConnection({userId:"new-width-owner"});
 expect(useAppStore.getState()).toMatchObject({sidebarWidth:296,workspaceWidth:null,resizingPane:null});
});

it("restores session tabs, selection and collapse separately from Files and empty sessions", () => {
  const store = useAppStore.getState();
  const session = (sessionId: string) => ({ scope: "actor", context: { kind: "session" as const, sessionId } });
  store.setWorkspaceContext(session("a"), true);
  store.openWorkspaceWindow(computer("one"));
  store.openWorkspaceWindow({ kind: "file", fileId: "report" });
  store.selectWorkspaceWindow(key("one"));
  store.setWorkspaceOpen(false);
  store.setWorkspaceContext(session("b"), true);
  expect(useAppStore.getState().workspaceWindows).toEqual([]);
  store.openWorkspaceWindow(computer("two"));
  store.setWorkspaceContext({ scope: "actor", context: { kind: "files" } }, true);
  store.openWorkspaceWindow({ kind: "file", fileId: "library" });
  store.setWorkspaceContext(session("a"), true);
  expect(useAppStore.getState()).toMatchObject({ workspaceActiveKey: key("one"), workspaceOpen: false });
  expect(useAppStore.getState().workspaceWindows).toHaveLength(2);
  store.setWorkspaceContext(session("b"), true);
  expect(useAppStore.getState()).toMatchObject({ workspaceWindows: [computer("two")], workspaceOpen: true });
  store.setWorkspaceContext(session("a"), false);
  expect(useAppStore.getState().workspaceOpen).toBe(false);
});

it("validates persisted descriptors and never restores credentials or runtime fields", () => {
  const owner = workspaceOwnerKey({ scope: "actor", context: { kind: "files" } });
  const restored = restoreWorkspaces({ [owner]: {
    workspaceWindows: [computer("a"), computer("a"), { kind: "webpage", id: "unsafe", url: "https://user:pass@example.test" }, { kind: "file", fileId: "ok", token: "secret" }],
    workspaceActiveKey: "missing", workspaceOpen: true, workspaceExpanded: true,
  }, bad: { workspaceWindows: [computer("leak")] } });
  expect(restored).toEqual({ [owner]: { workspaceWindows: [computer("a"), { kind: "file", fileId: "ok" }], workspaceActiveKey: key("a"), workspaceOpen: true } });
});

it("keeps reading positions in their owning session and applies a new citation to an existing tab", () => {
  const store = useAppStore.getState();
  const first = { scope: "actor", context: { kind: "session" as const, sessionId: "one" } };
  const second = { scope: "actor", context: { kind: "session" as const, sessionId: "two" } };
  const window = { kind: "sessionFile" as const, sessionId: "one", path: "code.rs" };
  const id = workspaceWindowKey(window);
  const position = { kind: "text" as const, resourcePath: "code.rs", line: 24, encoding: "utf-8" as const };
  store.setWorkspaceContext(first, true); store.openWorkspaceWindow(window);
  store.setWorkspaceContext(second, true); store.openWorkspaceWindow({ kind: "file", fileId: "other" });
  store.setWorkspacePreviewPosition(workspaceOwnerKey(first), id, position);
  expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "file", fileId: "other" }]);
  store.setWorkspaceContext(first, true);
  expect(useAppStore.getState().workspaceWindows[0]).toEqual({ ...window, position });
  store.openWorkspaceWindow({ ...window, position: { ...position, line: 40 } });
  expect(useAppStore.getState().workspaceWindows).toEqual([{ ...window, position: { ...position, line: 40 } }]);
  const restored = restoreWorkspaces({ [workspaceOwnerKey(first)]: useAppStore.getState() });
  expect(restored[workspaceOwnerKey(first)].workspaceWindows).toEqual(useAppStore.getState().workspaceWindows);
});

it("records hidden-session redirects in the owning snapshot", () => {
  const store = useAppStore.getState();
  store.createWorkspaceWebpage(); const key = useAppStore.getState().workspaceActiveKey!;
  store.setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "other" } }, true);
  store.navigateWorkspaceWebpage(ownerKey, key, "https://example.test/redirected", "Redirected");
  expect(useAppStore.getState().workspaceWindows).toEqual([]);
  store.setWorkspaceContext(owner, true);
  expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({ url: "https://example.test/redirected", title: "Redirected" });
});

it("persists active reading progress and redirects before a reload or context switch", () => {
  const store = useAppStore.getState();
  const file = { kind: "file" as const, fileId: "pdf" };
  const position = { kind: "pdf" as const, resourcePath: "preview.pdf", page: 2, scale: 1.25 };
  store.openWorkspaceWindow(file);
  store.setWorkspacePreviewPosition(ownerKey, workspaceWindowKey(file), position);
  store.createWorkspaceWebpage();
  store.navigateWorkspaceWebpage(ownerKey, useAppStore.getState().workspaceActiveKey!, "https://example.test/redirected", "Report");
  const restored = restoreWorkspaces(useAppStore.getState().workspaceSnapshots)[ownerKey];
  expect(restored.workspaceWindows[0]).toEqual({ ...file, position });
  expect(restored.workspaceWindows[1]).toMatchObject({ url: "https://example.test/redirected", title: "Report" });
});
