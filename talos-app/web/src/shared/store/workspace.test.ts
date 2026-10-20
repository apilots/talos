// @vitest-environment jsdom
import { beforeEach, expect, it } from "vitest";
import { useAppStore } from "./app-store";
import { normalizeWorkspaceWebpageUrl, workspaceWindowKey } from "./workspace-window";

const computer = (nodeId: string) => ({ kind: "computer" as const, nodeId });
const key = (nodeId: string) => workspaceWindowKey(computer(nodeId));
beforeEach(() => {
  useAppStore.getState().setConnection({ userId: "workspace-review", controlPlaneEndpoint: "http://localhost:8080" });
  useAppStore.setState({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
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
  const store=useAppStore.getState();store.openWorkspaceWindow({kind:"markdown",fileId:"owned-file"});
  store.createWorkspaceWebpage();const first=useAppStore.getState().workspaceActiveKey!;
  store.createWorkspaceWebpage();const second=useAppStore.getState().workspaceActiveKey!;
  expect(first).not.toBe(second);
  expect(useAppStore.getState().workspaceWindows.filter(window=>window.kind==="webpage")).toHaveLength(2);
  store.navigateWorkspaceWebpage(first,"https://example.test");store.navigateWorkspaceWebpage(second,"https://example.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(3);
  expect(useAppStore.getState().workspaceActiveKey).toBe(second);
  expect(useAppStore.getState().workspaceWindows.filter(window=>window.kind==="webpage").map(window=>window.url)).toEqual(["https://example.test/","https://example.test/"]);
  store.selectWorkspaceWindow(first);store.navigateWorkspaceWebpage(first,"http://localhost:3010/");
  expect(useAppStore.getState().workspaceActiveKey).toBe(first);
  store.closeWorkspaceWindow(first);store.navigateWorkspaceWebpage(first,"https://ignored.test/");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(2);
});
it("rejects unsafe or credential-bearing navigation without changing a blank tab", () => {
  const store=useAppStore.getState();store.createWorkspaceWebpage();const key=useAppStore.getState().workspaceActiveKey!;
  for (const url of ["javascript:alert(1)", "file:///etc/passwd", "https://user:secret@example.test", "not a URL"]) {
    expect(normalizeWorkspaceWebpageUrl(url)).toBeNull();
    expect(()=>store.navigateWorkspaceWebpage(key,url)).toThrow();
  }
  expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({kind:"webpage",url:""});
});
it("retires every descriptor on account or endpoint changes, but not credential rotation", () => {
  const store = useAppStore.getState();store.openWorkspaceWindow(computer("a"));
  store.setConnection({ authToken: "rotation" });expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  store.setConnection({ userId: "other-account" });expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
  store.openWorkspaceWindow({ kind: "markdown", fileId: "private" });
  store.setConnection({ controlPlaneEndpoint: "http://localhost:8081" });
  expect(useAppStore.getState()).toMatchObject({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
});

it("retains pane preferences through collapse and clears them with the account scope",()=>{
 const store=useAppStore.getState();store.setSidebarWidth(340);store.setWorkspaceWidth(700);store.openWorkspaceWindow(computer("a"));store.setWorkspaceOpen(false);
 expect(useAppStore.getState()).toMatchObject({sidebarWidth:340,workspaceWidth:700});
 store.setResizingPane("sidebar");store.setConnection({userId:"new-width-owner"});
 expect(useAppStore.getState()).toMatchObject({sidebarWidth:296,workspaceWidth:null,resizingPane:null});
});
