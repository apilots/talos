import "@testing-library/jest-dom/vitest";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { MemoryRouter, Routes, Route, Link } from "react-router-dom";
import { StrictMode, useEffect, type ReactNode } from "react";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { AppLayout } from "./AppLayout";
const fixture = vi.hoisted(() => ({ browserRequest: vi.fn(async()=>null), wide: true, selectedId: "selected-worker" as string | undefined, available: true, mounted: [] as string[], closed: [] as string[], entries: [] as string[] }));
vi.mock("@/features/computers", () => ({
  ComputerRail: () => <nav aria-label="Computer rail" />,
  useComputers: () => ({ selected: { available: fixture.available } }), useSelectedComputerId: () => fixture.selectedId, useProductScope: () => "account",
  useComputer: (nodeId: string) => ({ data: { displayName: nodeId, available: true } }),
  ComputerDesktopButton: ({ nodeId, label }: { nodeId?: string; label?: string }) => nodeId ? <button onClick={() => { fixture.entries.push(nodeId); useAppStore.getState().openWorkspaceWindow({ kind: "computer", nodeId }); }}>{label ?? "View computer"}</button> : null,
  ComputerDesktop: ({ nodeId }: { nodeId: string }) => {
    useEffect(() => { fixture.mounted.push(nodeId); return () => { fixture.closed.push(nodeId); }; }, [nodeId]);
    return <section aria-label="Computer viewer">{nodeId}<button>Worker action</button></section>;
  },
}));
vi.mock("@/pages/home/ComputerHomePage", () => ({ ComputerHomePage: () => <h1>Choose a computer</h1> }));
vi.mock("@/features/chat", () => ({
  SidebarContainer: ({ computerEntry }: { computerEntry?: ReactNode }) => <aside>Project and session sidebar{computerEntry}<button>Sidebar action</button></aside>,
  useSessionQuery: (id: string) => ({ data: id ? { title: "Bound session", projectId: "bound-project", nodeId: "session-worker" } : undefined }),
  useProjectsQuery: () => ({ data: [{ id: "bound-project", name: "Bound project", origin: "remote", updatedAt: 1 }] }),
}));
vi.mock("@/features/files", () => ({ useFileMetadata: () => ({ data: { originalFilename: "review.md" } }), MarkdownFilePreview: () => <section aria-label="Markdown preview">Actual file</section> }));
vi.mock("@/features/web-preview", () => ({ DesktopBrowserPage: () => <section aria-label="Native webpage" />, desktopBrowserRequest: fixture.browserRequest, useDesktopBrowserWorkspace: () => {}, useWorkspaceWebpageCreation: () => ({create: useAppStore.getState().createWorkspaceWebpage, disabled:false,error:false}), OpenWebpageButton: ({ icon }: { icon: string }) => <button>{icon === "plus" ? "New webpage tab" : "Open webpage preview"}</button>, WebPagePreview: ({ url }: { url: string }) => <section aria-label="Webpage preview">{url}<input aria-label="Preview address" /><iframe title="Preview content" /></section> }));
beforeEach(() => {
  fixture.browserRequest.mockReset().mockResolvedValue(null);
  fixture.wide = true; fixture.selectedId = "selected-worker"; fixture.available = true; fixture.mounted = []; fixture.closed = []; fixture.entries = [];
  vi.stubGlobal("matchMedia", () => ({ get matches() { return fixture.wide; }, addEventListener: vi.fn(), removeEventListener: vi.fn() }));
  HTMLElement.prototype.setPointerCapture=vi.fn();HTMLElement.prototype.hasPointerCapture=vi.fn(()=>true);HTMLElement.prototype.releasePointerCapture=vi.fn();
  HTMLDialogElement.prototype.showModal = function () { this.open = true;this.querySelector<HTMLElement>("button,input,[tabindex]")?.focus(); };
  HTMLDialogElement.prototype.close = function () { this.open = false;if(this.contains(document.activeElement)) (document.activeElement as HTMLElement).blur(); };
  useAppStore.setState({ sidebarOpen: false, desktopBrowser: null, workspaceFocusRevision: 0, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false,sidebarWidth:296,workspaceWidth:null,resizingPane:null });
});
afterEach(() => { cleanup(); vi.unstubAllGlobals();vi.restoreAllMocks(); });
export function show(path = "/sessions/session", strict = false) {
  const content=<MemoryRouter initialEntries={[path]}><Routes><Route element={<AppLayout />}><Route path="*" element={<div>Page content<Link to="/settings/mcp-servers">Settings</Link><Link to="/files">Files page</Link></div>} /></Route></Routes></MemoryRouter>;
  return render(strict ? <StrictMode>{content}</StrictMode> : content);
}
export function open(nodeId: string) { act(() => useAppStore.getState().openWorkspaceWindow({ kind: "computer", nodeId })); }

export { fixture };
