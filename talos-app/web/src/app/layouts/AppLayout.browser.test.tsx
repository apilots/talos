// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MemoryRouter, Routes, Route, Link } from "react-router-dom";
import { useEffect, type ReactNode } from "react";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { workspaceWindowKey } from "@/shared/store/workspace-window";
import { AppLayout } from "./AppLayout";
const fixture = vi.hoisted(() => ({ wide: true, mounted: [] as string[], closed: [] as string[], entries: [] as string[] }));
vi.mock("@/features/computers", () => ({
  ComputerRail: () => <nav aria-label="Computer rail" />,
  useComputers: () => ({ selected: { available: true } }), useSelectedComputerId: () => "selected-worker", useProductScope: () => "account",
  useComputer: (nodeId: string) => ({ data: { displayName: nodeId, available: true } }),
  ComputerDesktopButton: ({ nodeId, label }: { nodeId?: string; label?: string }) => nodeId ? <button onClick={() => { fixture.entries.push(nodeId); useAppStore.getState().openWorkspaceWindow({ kind: "computer", nodeId }); }}>{label ?? "View computer"}</button> : null,
  ComputerDesktop: ({ nodeId }: { nodeId: string }) => {
    useEffect(() => { fixture.mounted.push(nodeId); return () => { fixture.closed.push(nodeId); }; }, [nodeId]);
    return <section aria-label="Computer viewer">{nodeId}<button>Worker action</button></section>;
  },
}));
vi.mock("@/features/chat", () => ({
  SidebarContainer: ({ computerEntry }: { computerEntry?: ReactNode }) => <aside>Project and session sidebar{computerEntry}<button>Sidebar action</button></aside>,
  useSessionQuery: (id: string) => ({ data: id ? { title: "Bound session", projectId: "bound-project", nodeId: "session-worker" } : undefined }),
  useProjectsQuery: () => ({ data: [{ id: "bound-project", name: "Bound project", origin: "remote", updatedAt: 1 }] }),
}));
vi.mock("@/features/files", () => ({ useFileMetadata: () => ({ data: { originalFilename: "review.md" } }), MarkdownFilePreview: () => <section aria-label="Markdown preview">Actual file</section> }));
vi.mock("@/features/web-preview", () => ({ OpenWebpageButton: ({ icon }: { icon: string }) => <button>{icon === "plus" ? "New webpage tab" : "Open webpage preview"}</button>, WebPagePreview: ({ url }: { url: string }) => <section aria-label="Webpage preview">{url}</section> }));
beforeEach(() => {
  fixture.wide = true; fixture.mounted = []; fixture.closed = []; fixture.entries = [];
  vi.stubGlobal("matchMedia", () => ({ matches: fixture.wide, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  HTMLElement.prototype.setPointerCapture=vi.fn();HTMLElement.prototype.hasPointerCapture=vi.fn(()=>true);HTMLElement.prototype.releasePointerCapture=vi.fn();
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
  useAppStore.setState({ sidebarOpen: false, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false,sidebarWidth:296,workspaceWidth:null,resizingPane:null });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function show(path = "/sessions/session") {
  return render(<MemoryRouter initialEntries={[path]}><Routes><Route element={<AppLayout />}><Route path="*" element={<div>Page content<Link to="/settings/mcp-servers">Settings</Link><Link to="/files">Files page</Link></div>} /></Route></Routes></MemoryRouter>);
}
function open(nodeId: string) { act(() => useAppStore.getState().openWorkspaceWindow({ kind: "computer", nodeId })); }

it("has one outer navigation and routes the Session computer entry to its bound node", () => {
  show();
  expect(screen.getAllByRole("banner", { name: "Application navigation" })).toHaveLength(1);
  expect(screen.getAllByRole("button", { name: "Back" })).toHaveLength(1);
  expect(screen.getByText("Bound session")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
  fireEvent.click(screen.getByRole("button", { name: "Your computer" }));
  expect(fixture.entries).toEqual(["session-worker"]);
  expect(screen.getByRole("region", { name: "Computer viewer" })).toHaveTextContent("session-worker");
});
it("moves sidebar toggling and project navigation into the outer header", () => {
  show();fireEvent.click(screen.getByRole("button", { name: "Open sidebar" }));
  expect(screen.getByText("Project and session sidebar")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Collapse sidebar" })).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: "Collapse sidebar" }));
  expect(screen.getByText("Project and session sidebar").closest('[aria-label="Projects and chats"]')).toHaveAttribute("inert");
  fireEvent.click(screen.getByRole("button", { name: "Bound project" }));
  expect(useAppStore.getState().activeProjectId).toBe("bound-project");
  expect(screen.getByText("New chat")).toBeInTheDocument();
});
it("retains opened workspace content on Settings without adding an MCP business entry", () => {
  show();open("worker");fireEvent.click(screen.getByRole("link", { name: "Settings" }));
  expect(screen.getByRole("region", { name: "Computer viewer" })).toHaveTextContent("worker");
  expect(screen.queryByRole("button", { name: "View computer" })).not.toBeInTheDocument();
  expect(fixture.closed).toHaveLength(0);
  fireEvent.click(screen.getByRole("button", { name: "Hide workspace (1 open windows)" }));
  expect(screen.getByRole("button", { name: "Show workspace (1 open windows)" })).toBeEnabled();
});
it("counts retained windows honestly, deduplicates openings, selects and closes them", () => {
  show();expect(screen.getByRole("button", { name: "New workspace tab" })).toBeEnabled();
  open("a");open("b");open("a");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(2);
  fireEvent.click(screen.getByRole("button", { name: "Hide workspace (2 open windows)" }));
  expect(screen.queryByRole("region", { name: "Computer viewer" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Show workspace (2 open windows)" }));
  fireEvent.click(screen.getByRole("tab", { name: "b" }));
  expect(screen.getByRole("region", { name: "Computer viewer" })).toHaveTextContent("b");
  fireEvent.click(screen.getByRole("button", { name: "Close a" }));
  expect(screen.getByRole("region", { name: "Computer viewer" })).toHaveTextContent("b");
  fireEvent.click(screen.getByRole("button", { name: "Close b" }));
  expect(screen.getByRole("button", { name: "New workspace tab" })).toBeEnabled();
});
it("expands and restores geometry with one interactive topbar and no content remount", () => {
  show();open("a");
  fireEvent.click(screen.getByRole("button", { name: "Expand workspace" }));
  const panel = screen.getByRole("dialog", { name: "Right workspace" });
  expect(within(panel).queryByRole("banner", { name: "Application navigation" })).not.toBeInTheDocument();
  expect(within(panel).getByRole("banner",{name:"Workspace controls"})).toContainElement(within(panel).getByRole("tablist"));
  expect(screen.getAllByRole("button", { name: "Hide workspace (1 open windows)" })).toHaveLength(1);
  fireEvent.click(within(panel).getByRole("button", { name: "Restore workspace" }));
  expect(fixture.mounted).toEqual(["a"]);expect(fixture.closed).toHaveLength(0);
});
it("collapses from a modal with Escape, keeping count and returning focus outside the hidden content", () => {
  fixture.wide = false;show();open("a");
  const panel = screen.getByRole("dialog", { name: "Right workspace" });
  within(panel).getByRole("button", { name: "Worker action" }).focus();
  fireEvent(panel, new Event("cancel", { bubbles: false, cancelable: true }));
  expect(screen.queryByRole("dialog", { name: "Right workspace" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Show workspace (1 open windows)" })).toHaveFocus();
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  expect(fixture.closed).toEqual(["a"]);
});
it("assembles real Markdown and URL descriptors independently from Computer content", () => {
  show();open("worker");
  act(() => useAppStore.getState().openWorkspaceWindow({ kind: "markdown", fileId: "owned-file" }));
  expect(screen.getByRole("region", { name: "Markdown preview" })).toBeInTheDocument();
  expect(fixture.closed).toEqual(["worker"]);
  act(() => { useAppStore.getState().createWorkspaceWebpage(); useAppStore.getState().navigateWorkspaceWebpage(useAppStore.getState().workspaceActiveKey!, "http://localhost:3010/"); });
  expect(screen.getByRole("region", { name: "Webpage preview" })).toHaveTextContent("http://localhost:3010/");
  fireEvent.click(screen.getByRole("tab", { name: "review.md" }));
  expect(useAppStore.getState().workspaceActiveKey).toBe(workspaceWindowKey({ kind: "markdown", fileId: "owned-file" }));
  expect(screen.getByRole("region", { name: "Markdown preview" })).toBeInTheDocument();
});

it("keeps the single application navigation operable inside the narrow sidebar modal", () => {
 fixture.wide=false;show();
 fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
 const panel=screen.getByRole("dialog",{name:"Projects and chats"});
 expect(within(panel).getByRole("button",{name:"Collapse sidebar"})).toBeInTheDocument();
 expect(screen.getAllByRole("banner",{name:"Application navigation"})).toHaveLength(1);
 fireEvent(panel,new Event("cancel",{bubbles:false,cancelable:true}));
 expect(screen.getByRole("button",{name:"Open sidebar"})).toHaveFocus();
 expect(screen.queryByRole("dialog",{name:"Projects and chats"})).not.toBeInTheDocument();
});

it("uses the existing router history for application back and forward navigation", () => {
 show();fireEvent.click(screen.getByRole("link",{name:"Files page"}));
 expect(within(screen.getByRole("banner",{name:"Application navigation"})).getByText("Files")).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"Back"}));
 expect(screen.getByText("Bound session")).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"Forward"}));
 expect(within(screen.getByRole("banner",{name:"Application navigation"})).getByText("Files")).toBeInTheDocument();
});

it("uses browser-style tab keyboard navigation and one new-tab entry", () => {
 show();expect(screen.getAllByRole("button",{name:/Open webpage preview|New webpage tab/})).toHaveLength(1);
 open("a");open("b");open("c");
 const tabs=screen.getByRole("tablist",{name:"Open windows"});
 expect(screen.getAllByRole("button",{name:/Open webpage preview|New webpage tab/})).toHaveLength(1);
 expect(within(tabs).getAllByRole("tab")).toHaveLength(3);
 expect(within(tabs.parentElement!).getByRole("button",{name:"New webpage tab"})).toBeInTheDocument();
 expect(within(screen.getByRole("banner",{name:"Application navigation"})).queryByRole("button",{name:/Open webpage preview|New webpage tab/})).not.toBeInTheDocument();
 expect(screen.getByRole("tab",{name:"c"})).toHaveAttribute("aria-selected","true");
 fireEvent.keyDown(screen.getByRole("tab",{name:"c"}),{key:"ArrowRight"});
 expect(screen.getByRole("tab",{name:"a"})).toHaveFocus();
 expect(screen.getByRole("tabpanel",{name:"a"})).toHaveTextContent("a");
 fireEvent.keyDown(screen.getByRole("tab",{name:"a"}),{key:"End"});
 expect(screen.getByRole("tab",{name:"c"})).toHaveFocus();
 fireEvent.keyDown(screen.getByRole("tab",{name:"c"}),{key:"ArrowLeft"});
 expect(screen.getByRole("tab",{name:"b"})).toHaveFocus();
 fireEvent.keyDown(screen.getByRole("tab",{name:"b"}),{key:"Home"});
 expect(screen.getByRole("tab",{name:"a"})).toHaveFocus();
 fireEvent.click(screen.getByRole("button",{name:"Close a"}));
 expect(screen.getByRole("tab",{name:"b"})).toHaveFocus();
 expect(screen.getByRole("tab",{name:"b"})).toHaveAttribute("tabindex","0");
 expect(screen.getByRole("tab",{name:"c"})).toHaveAttribute("tabindex","-1");
 fireEvent.click(screen.getByRole("button",{name:"Hide workspace (2 open windows)"}));
 expect(screen.getAllByRole("button",{name:/Open webpage preview|New webpage tab/})).toHaveLength(1);
});

it("keeps middle navigation in its inert region while the modal workspace owns only resources", () => {
 useAppStore.setState({sidebarOpen:true});show();open("worker");
 const business=screen.getByRole("region",{name:"Business area"});
 fireEvent.click(screen.getByRole("button",{name:"Expand workspace"}));
 expect(business).toHaveAttribute("inert");expect(business).toHaveAttribute("aria-hidden","true");
 const panel=screen.getByRole("dialog",{name:"Right workspace"});
 expect(within(panel).queryByRole("button",{name:/sidebar/})).not.toBeInTheDocument();
 fireEvent.click(within(panel).getByRole("button",{name:"Restore workspace"}));
 expect(business).not.toHaveAttribute("inert");expect(screen.getByRole("button",{name:"Collapse sidebar"})).toBeInTheDocument();
 expect(fixture.closed).toHaveLength(0);
});

it("aligns three full-height regions and keeps business versus workspace controls in their own headers",()=>{
 useAppStore.setState({sidebarOpen:true});show();open("worker");
 const business=screen.getByRole("region",{name:"Business area"});
 const workspace=screen.getByRole("dialog",{name:"Right workspace"});
 expect(workspace.parentElement).toBe(business.parentElement);
 expect(within(business).getByRole("main")).toBeInTheDocument();
 expect(within(business).getByRole("button",{name:"Your computer"})).toBeInTheDocument();
 expect(within(business).getByRole("banner",{name:"Application navigation"})).not.toContainElement(screen.getByRole("button",{name:"Your computer"}));
 expect(within(business).getByText("Bound session")).toBeInTheDocument();
 expect(within(business).queryByRole("group",{name:"Workspace controls"})).not.toBeInTheDocument();
 const controls=within(workspace).getByRole("banner",{name:"Workspace controls"});
 expect(within(controls).getByRole("button",{name:"Hide workspace (1 open windows)"})).toBeInTheDocument();
 expect(within(controls).queryByText("Bound session")).not.toBeInTheDocument();
 expect(within(controls).queryByRole("button",{name:"View computer"})).not.toBeInTheDocument();
 expect(within(controls).getByRole("button",{name:"Hide workspace (1 open windows)"}).querySelector(".lucide-panel-right")).not.toBeNull();
 fireEvent.click(screen.getByRole("button",{name:"Collapse sidebar"}));
 fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
 expect(fixture.mounted).toEqual(["worker"]);expect(fixture.closed).toHaveLength(0);
});
it("uses one animated sidebar width for the header/body split and makes collapsed controls immediately accessible",()=>{
 useAppStore.setState({sidebarOpen:true});show();
 const business=screen.getByRole("region",{name:"Business area"});
 const sidebarHeader=within(business).getByRole("banner",{name:"Sidebar navigation"});
 const mainHeader=within(business).getByRole("banner",{name:"Application navigation"});
 expect(within(sidebarHeader).getByRole("button",{name:"Back"})).toBeInTheDocument();
 expect(within(sidebarHeader).queryByText("Bound session")).not.toBeInTheDocument();
 expect(within(mainHeader).getByText("Bound session")).toBeInTheDocument();
 expect(within(mainHeader).queryByRole("button",{name:"Back"})).not.toBeInTheDocument();
 expect(business.style.getPropertyValue("--sidebar-width")).toBe("296px");
 const headerGrid=sidebarHeader.parentElement!.parentElement!;
 const bodyGrid=within(business).getByRole("main").parentElement!;
 for(const grid of [headerGrid,bodyGrid]){
  expect(grid.className).toContain("grid-cols-[var(--sidebar-width)_minmax(0,1fr)]");
  expect(grid.className).toContain("duration-200");expect(grid.className).toContain("motion-reduce:transition-none");
 }
 within(sidebarHeader).getByRole("button",{name:"Collapse sidebar"}).focus();
 fireEvent.click(within(sidebarHeader).getByRole("button",{name:"Collapse sidebar"}));
 expect(business.style.getPropertyValue("--sidebar-width")).toBe("0px");
 expect(screen.getByRole("button",{name:"Open sidebar"})).toHaveFocus();
 expect(screen.getByText("Project and session sidebar").closest('[aria-label="Projects and chats"]')).toHaveAttribute("inert");
 const drawer=screen.getByText("Project and session sidebar").parentElement!;
 expect(drawer.className).toContain("-translate-x-full");expect(drawer.className).toContain("w-[var(--sidebar-open-width)]");
 expect(screen.getByRole("banner",{name:"Sidebar navigation"})).toBe(sidebarHeader);
 expect(screen.getByText("Bound session")).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
 expect(business.style.getPropertyValue("--sidebar-width")).toBe("296px");
 expect(screen.getByRole("button",{name:"Collapse sidebar"})).toHaveFocus();
});

it("immediately retires sidebar focus on close without waiting for its visual transition",()=>{
 useAppStore.setState({sidebarOpen:true});show();
 const action=screen.getByRole("button",{name:"Sidebar action"});action.focus();
 act(()=>useAppStore.getState().setSidebarOpen(false));
 expect(action.closest('[aria-label="Projects and chats"]')).toHaveAttribute("inert");
 expect(screen.getByRole("button",{name:"Open sidebar"})).toHaveFocus();
});

it("uses an ordinary wide sidebar while the workspace toggles, keeping its content mounted",()=>{
 useAppStore.setState({sidebarOpen:true});show();
 const sidebar=screen.getByRole("complementary",{name:"Projects and chats"});
 expect(sidebar.tagName).toBe("ASIDE");expect(sidebar).not.toHaveAttribute("open");
 open("worker");fireEvent.click(screen.getByRole("button",{name:"Hide workspace (1 open windows)"}));
 expect(screen.getByRole("complementary",{name:"Projects and chats"})).toBe(sidebar);
 expect(within(sidebar).getByRole("button",{name:"Sidebar action"})).toBeInTheDocument();
 expect(screen.queryByRole("dialog",{name:"Projects and chats"})).not.toBeInTheDocument();
});
it("creates a real empty webpage from the zero-tab Plus and only toggles retained nonzero tabs",()=>{
 show();
 const create=screen.getByRole("button",{name:"New workspace tab"});
 expect(create).toBeEnabled();expect(create.querySelector(".lucide-plus")).not.toBeNull();
 expect(screen.getByRole("button",{name:"Open webpage preview"})).toBeInTheDocument();
 fireEvent.click(create);
 expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
 expect(useAppStore.getState().workspaceWindows[0]).toMatchObject({kind:"webpage",url:""});
 const hide=screen.getByRole("button",{name:"Hide workspace (1 open windows)"});
 expect(hide).toHaveTextContent("1");expect(hide.querySelector(".lucide-panel-right")).not.toBeNull();
 fireEvent.click(hide);
 const showWorkspace=screen.getByRole("button",{name:"Show workspace (1 open windows)"});
 expect(showWorkspace).toHaveTextContent("1");fireEvent.click(showWorkspace);
 expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
 fireEvent.click(screen.getByRole("button",{name:"Close New webpage tab"}));
 expect(screen.getByRole("button",{name:"New workspace tab"})).toBeEnabled();
});

it("resizes both boundaries without remounting Computer content and keeps the 1024px main area usable",()=>{
 useAppStore.setState({sidebarOpen:true});show();open("worker");
 const business=screen.getByRole("region",{name:"Business area"});
 const sidebar=screen.getByRole("separator",{name:"Resize sidebar"});
 expect(sidebar).toHaveAttribute("aria-valuenow","288");
 fireEvent.pointerDown(sidebar,{pointerId:1,button:0,clientX:300});
 expect(useAppStore.getState().resizingPane).toBe("sidebar");
 fireEvent.pointerMove(sidebar,{pointerId:1,clientX:236});
 expect(business.style.getPropertyValue("--sidebar-width")).toBe("224px");
 expect(within(business).getByRole("main").parentElement!.className).not.toContain("duration-200");
 fireEvent.pointerUp(sidebar,{pointerId:1});expect(useAppStore.getState().resizingPane).toBeNull();
 const workspace=screen.getByRole("separator",{name:"Resize workspace"});
 fireEvent.keyDown(workspace,{key:"End"});expect(useAppStore.getState().workspaceWidth).toBe(424);
 expect(968-useAppStore.getState().sidebarWidth-useAppStore.getState().workspaceWidth!).toBeGreaterThanOrEqual(320);
 expect(fixture.mounted).toEqual(["worker"]);expect(fixture.closed).toHaveLength(0);
});

it("does not reopen a covered narrow sidebar after the workspace closes",()=>{
 fixture.wide=false;useAppStore.setState({sidebarOpen:true});show();open("worker");
 expect(useAppStore.getState().sidebarOpen).toBe(false);
 const panel=screen.getByRole("dialog",{name:"Right workspace"});
 within(panel).getByRole("button",{name:"Worker action"}).focus();
 fireEvent(panel,new Event("cancel",{cancelable:true}));
 expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
 expect(screen.getByRole("button",{name:/^Show workspace/})).toHaveFocus();
});
