// @vitest-environment jsdom
import { workspaceOwnerKey } from "@/shared/store/workspace-state";
import "@testing-library/jest-dom/vitest";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { fixture, show, open } from "./AppLayout.test-utils";
import { workspaceWindowKey } from "@/shared/store/workspace-window";

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
it("binds previews opened on a fresh Settings page to the general workspace", () => {
  show("/settings/mcp-servers");
  act(() => useAppStore.getState().createWorkspaceWebpage());
  const state = useAppStore.getState();
  expect(state.workspaceOwner).toEqual({ scope: "account", context: { kind: "general" } });
  expect(Object.values(state.workspaceSnapshots)).toContainEqual({
    workspaceWindows: state.workspaceWindows, workspaceActiveKey: state.workspaceActiveKey, workspaceOpen: true,
  });
});
it("binds right tabs to sessions and Files while retaining hidden webpage instances", () => {
  show();
  act(() => { useAppStore.getState().createWorkspaceWebpage(); useAppStore.getState().navigateWorkspaceWebpage(workspaceOwnerKey(useAppStore.getState().workspaceOwner!),useAppStore.getState().workspaceActiveKey!, "https://one.test/"); });
  const frame = screen.getByTitle("Preview content") as HTMLIFrameElement;
  const document = frame.contentDocument!;
  document.body.innerHTML = '<input value="Retained session draft">';
  fireEvent.click(screen.getByRole("link", { name: "Second session" }));
  expect(useAppStore.getState().workspaceWindows).toEqual([]);
  expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  expect(frame.isConnected).toBe(true);
  open("second-worker");
  fireEvent.click(screen.getByRole("link", { name: "Files page" }));
  expect(useAppStore.getState().workspaceWindows).toEqual([]);
  open("library-worker");
  fireEvent.click(screen.getByRole("link", { name: "First session" }));
  expect(screen.getByRole("region", { name: "Webpage preview" })).toContainElement(frame);
  expect(frame.contentDocument).toBe(document);
  expect(frame.contentDocument?.querySelector("input")).toHaveValue("Retained session draft");
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  expect(screen.queryByRole("tab", { name: "library-worker" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("link", { name: "Second session" }));
  expect(screen.getByRole("tab", { name: "second-worker" })).toHaveAttribute("aria-selected", "true");
});
it("scrolls a restored active tab into view without taking focus from session navigation", () => {
  const previous = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollIntoView");
  const scroll = vi.fn();
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scroll });
  try {
    show();open("first-worker");
    fireEvent.click(screen.getByRole("link", { name: "Second session" }));open("second-worker");
    const navigation = screen.getByRole("link", { name: "First session" });
    navigation.focus();fireEvent.click(navigation);
    expect(navigation).toHaveFocus();
    expect(scroll.mock.contexts.at(-1)).toBe(screen.getByRole("tab", { name: "first-worker" }));
  } finally {
    if (previous) Object.defineProperty(HTMLElement.prototype, "scrollIntoView", previous);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollIntoView");
  }
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
  act(() => useAppStore.getState().openWorkspaceWindow({ kind: "file", fileId: "owned-file" }));
  expect(screen.getByRole("region", { name: "Markdown preview" })).toBeInTheDocument();
  expect(fixture.closed).toEqual(["worker"]);
  act(() => { useAppStore.getState().createWorkspaceWebpage(); useAppStore.getState().navigateWorkspaceWebpage(workspaceOwnerKey(useAppStore.getState().workspaceOwner!),useAppStore.getState().workspaceActiveKey!, "http://localhost:3010/"); });
  expect(screen.getByRole("region", { name: "Webpage preview" })).toHaveTextContent("http://localhost:3010/");
  fireEvent.click(screen.getByRole("tab", { name: "review.md" }));
  expect(useAppStore.getState().workspaceActiveKey).toBe(workspaceWindowKey({ kind: "file", fileId: "owned-file" }));
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
 show();expect(screen.getByRole("button",{name:"New workspace tab"})).toBeInTheDocument();
 expect(screen.queryByRole("button",{name:/Open webpage preview|New webpage tab/})).not.toBeInTheDocument();
 open("a");open("b");open("c");
 const tabs=screen.getByRole("tablist",{name:"Open windows"});
 expect(screen.getAllByRole("button",{name:"New webpage tab"})).toHaveLength(1);
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
 expect(screen.queryByRole("button",{name:/Open webpage preview|New webpage tab/})).not.toBeInTheDocument();
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
 expect(screen.queryByRole("button",{name:"Open webpage preview"})).not.toBeInTheDocument();
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
it("retires interaction immediately but keeps the active webpage painted for the drawer exit",()=>{
 show();act(()=>useAppStore.getState().createWorkspaceWebpage());
 const page=screen.getByRole("region",{name:"Webpage preview"});
 const panel=page.closest('[role="tabpanel"]')!;
 const workspace=screen.getByRole("dialog",{name:"Right workspace"});
 fireEvent.click(screen.getByRole("button",{name:"Hide workspace (1 open windows)"}));
 expect(page).toBeInTheDocument();expect(panel).not.toHaveAttribute("hidden");
 expect(panel).toHaveAttribute("inert");expect(workspace).toHaveAttribute("inert");
 expect(workspace).toHaveAttribute("aria-hidden","true");
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
