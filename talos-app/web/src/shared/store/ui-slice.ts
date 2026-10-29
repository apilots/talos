import { createDefaultProjects, getProjectViewKey } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";
import { normalizeWorkspaceWebpageUrl, workspaceWindowKey, type FilePreviewPosition, type WorkspaceWindow } from "./workspace-window";
import { emptyWorkspace, snapshotWorkspace, workspaceBelongsToScope, workspaceOwnerKey, type WorkspaceOwner, type WorkspaceSnapshot } from "./workspace-state";
import { createUuid } from "@/shared/lib/uuid";
import type { DesktopBrowserState } from "@talos/sdk-app-server/desktop";

export type UiTheme = "dark" | "light";
export type Locale = "en" | "zh-CN";
export const SIDEBAR_WIDTH = { initial: 296, min: 224, max: 480 } as const;
export const WORKSPACE_WIDTH = { initial: 480, min: 360, max: 1040 } as const;
export type ResizingPane = "sidebar" | "workspace" | null;

export interface UiSlice {
  desktopBrowser: DesktopBrowserState | null;
  syncDesktopBrowser: (state: DesktopBrowserState | null) => void;
  workspaceFocusRevision: number;
  sidebarWidth: number;
  workspaceWidth: number | null;
  resizingPane: ResizingPane;
  setSidebarWidth: (width: number) => void;
  setWorkspaceWidth: (width: number | null) => void;
  setResizingPane: (pane: ResizingPane) => void;
  workspaceWindows: WorkspaceWindow[];
  workspaceActiveKey: string | null;
  workspaceOpen: boolean;
  workspaceExpanded: boolean;
  workspaceOwner: WorkspaceOwner | null;
  workspaceSnapshots: Record<string, WorkspaceSnapshot>;
  setWorkspaceContext: (owner: WorkspaceOwner, restoreOpen: boolean) => void;
  openWorkspaceWindow: (window: Exclude<WorkspaceWindow, { kind: "webpage" | "browser" }>) => void;
  createWorkspaceWebpage: () => void;
  openWorkspaceWebpage: (url: string) => void;
  selectWorkspaceWindow: (key: string) => void;
  closeWorkspaceWindow: (key: string) => void;
  navigateWorkspaceWebpage: (ownerKey: string, key: string, url: string, title?: string) => void;
  setWorkspacePreviewPosition: (ownerKey: string, key: string, position: FilePreviewPosition | undefined) => void;
  setWorkspaceOpen: (open: boolean) => void;
  setWorkspaceExpanded: (expanded: boolean) => void;
  uiTheme: UiTheme;
  locale: Locale;
  sidebarOpen: boolean;
  activeProjectId: string;
  setUiTheme: (theme: UiTheme) => void;
  setLocale: (locale: Locale) => void;
  setSidebarOpen: (open: boolean) => void;
  setActiveProjectId: (projectId: string) => void;
}

export type UiSliceSetter = (updater: (state: UiSlice) => Partial<UiSlice>) => void;

export const defaultProjects = createDefaultProjects();

export function normalizeProjects(projects?: ProjectView[]) {
  const normalized = new Map<string, ProjectView>();

  for (const project of defaultProjects) {
    normalized.set(getProjectViewKey(project), project);
  }

  for (const project of projects ?? []) {
    const key = getProjectViewKey(project);
    const current = normalized.get(key);
    if (!current) {
      normalized.set(key, project);
      continue;
    }

    normalized.set(key, {
      ...current,
      ...project,
      name: project.name || current.name,
      origin: current.origin === "default" ? current.origin : (project.origin ?? current.origin),
      sessionCount: project.sessionCount ?? current.sessionCount,
      updatedAt: Math.max(project.updatedAt, current.updatedAt),
    });
  }

  return [...normalized.values()].sort((left, right) => {
    if (left.origin === "default") {
      return -1;
    }
    if (right.origin === "default") {
      return 1;
    }
    return right.updatedAt - left.updatedAt;
  });
}

export function createUiSlice(set: UiSliceSetter): UiSlice {
  const updateWorkspace: UiSliceSetter = updater => set(state => {
    const next = updater(state);
    return { ...next, ...(state.workspaceOwner ? { workspaceSnapshots: {
      ...state.workspaceSnapshots, [workspaceOwnerKey(state.workspaceOwner)]: snapshotWorkspace({ ...state, ...next }),
    } } : {}) };
  });
  const updateOwnedWindows = (ownerKey: string, update: (windows: WorkspaceWindow[]) => WorkspaceWindow[]) => set(state => {
    const current = !!state.workspaceOwner && workspaceOwnerKey(state.workspaceOwner) === ownerKey;
    const snapshot = current ? snapshotWorkspace(state) : state.workspaceSnapshots[ownerKey];
    if (!snapshot) return {};
    const next = { ...snapshot, workspaceWindows: update(snapshot.workspaceWindows) };
    return { ...(current ? { workspaceWindows: next.workspaceWindows } : {}), workspaceSnapshots: { ...state.workspaceSnapshots, [ownerKey]: next } };
  });
  return {
    workspaceFocusRevision: 0,
    desktopBrowser: null,
    syncDesktopBrowser: browser => set(state => {
      const previous = state.desktopBrowser;
      if (browser && previous?.sessionId === browser.sessionId && browser.revision <= previous.revision) return {};
      const byId = new Map(browser?.tabs.map(tab => [tab.id, { ...tab, kind: "browser" as const }]) ?? []);
      const known = new Set(previous?.tabs.map(tab => tab.id));
      const reconcile = (snapshot: WorkspaceSnapshot, current: boolean): WorkspaceSnapshot => {
        const windows: WorkspaceWindow[] = snapshot.workspaceWindows.flatMap((window): WorkspaceWindow[] => {
          if (window.kind !== "browser") return [window];
          const tab = byId.get(window.id); return tab ? [tab] : [];
        });
        if (current) windows.push(...[...byId.values()].filter(tab => !known.has(tab.id) && !windows.some(window => window.kind === "browser" && window.id === tab.id)));
        const reveal = current && browser?.revealTabId ? windows.find(window => window.kind === "browser" && window.id === browser.revealTabId) : undefined;
        const selected = windows.find(window => workspaceWindowKey(window) === snapshot.workspaceActiveKey);
        const neighbor = windows[Math.max(0, Math.min(snapshot.workspaceWindows.findIndex(window => workspaceWindowKey(window) === snapshot.workspaceActiveKey), windows.length - 1))];
        return { workspaceWindows: windows, workspaceActiveKey: reveal ? workspaceWindowKey(reveal) : selected ? snapshot.workspaceActiveKey : neighbor ? workspaceWindowKey(neighbor) : null,
          workspaceOpen: windows.length > 0 && (Boolean(reveal) || snapshot.workspaceOpen) };
      };
      const next = reconcile(snapshotWorkspace(state), true);
      const reveal = next.workspaceActiveKey !== state.workspaceActiveKey && browser?.revealTabId && next.workspaceWindows.some(window => window.kind === "browser" && window.id === browser.revealTabId);
      const snapshots = Object.fromEntries(Object.entries(state.workspaceSnapshots).map(([key, snapshot]) => [key,
        state.workspaceOwner && workspaceBelongsToScope(key, state.workspaceOwner.scope) ? reconcile(snapshot, false) : snapshot]));
      if (state.workspaceOwner) snapshots[workspaceOwnerKey(state.workspaceOwner)] = next;
      return { ...next, workspaceSnapshots: snapshots, desktopBrowser: browser,
        workspaceFocusRevision: state.workspaceFocusRevision + (reveal ? 1 : 0), workspaceExpanded: next.workspaceWindows.length > 0 && state.workspaceExpanded };
    }),
    sidebarWidth: SIDEBAR_WIDTH.initial,
    workspaceWidth: null,
    resizingPane: null,
    setSidebarWidth: width => set(() => ({ sidebarWidth: Math.round(Math.min(SIDEBAR_WIDTH.max, Math.max(SIDEBAR_WIDTH.min, width))) })),
    setWorkspaceWidth: width => set(() => ({ workspaceWidth: width === null ? null : Math.round(Math.min(WORKSPACE_WIDTH.max, Math.max(WORKSPACE_WIDTH.min, width))) })),
    setResizingPane: resizingPane => set(() => ({ resizingPane })),
    workspaceWindows: [],
    workspaceActiveKey: null,
    workspaceOpen: false,
    workspaceExpanded: false,
    workspaceOwner: null,
    workspaceSnapshots: {},
    setWorkspaceContext: (owner, restoreOpen) => set(state => {
      const key = workspaceOwnerKey(owner);
      if (state.workspaceOwner && workspaceOwnerKey(state.workspaceOwner) === key) return {};
      const snapshot = state.workspaceSnapshots[key] ?? emptyWorkspace;
      return { ...snapshot, workspaceOwner: owner, workspaceOpen: restoreOpen && snapshot.workspaceOpen, workspaceExpanded: false };
    }),
    openWorkspaceWindow: (window) => updateWorkspace((state) => {
      const key = workspaceWindowKey(window);
      return {
        workspaceWindows: state.workspaceWindows.some(entry => workspaceWindowKey(entry) === key)
          ? (window.kind !== "computer" && window.position ? state.workspaceWindows.map(entry => workspaceWindowKey(entry) === key ? { ...entry, position: window.position } : entry) : state.workspaceWindows) : [...state.workspaceWindows, window],
        workspaceActiveKey: key,
        workspaceOpen: true,
        workspaceFocusRevision: state.workspaceFocusRevision + 1,
      };
    }),
    createWorkspaceWebpage: () => updateWorkspace((state) => {
      const window: WorkspaceWindow = { kind: "webpage", id: createUuid(), url: "" };
      return { workspaceWindows: [...state.workspaceWindows, window], workspaceActiveKey: workspaceWindowKey(window), workspaceOpen: true, workspaceFocusRevision: state.workspaceFocusRevision + 1 };
    }),
    openWorkspaceWebpage: url => updateWorkspace(state => {
      const address = normalizeWorkspaceWebpageUrl(url);
      if (!address) throw new Error("Enter an HTTP or HTTPS address without credentials.");
      const existing = state.workspaceWindows.find(window => window.kind === "webpage" && window.url === address);
      const window: WorkspaceWindow = existing ?? { kind: "webpage", id: createUuid(), url: address };
      return { workspaceWindows: existing ? state.workspaceWindows : [...state.workspaceWindows, window], workspaceActiveKey: workspaceWindowKey(window), workspaceOpen: true, workspaceFocusRevision: state.workspaceFocusRevision + 1 };
    }),
    selectWorkspaceWindow: (key) => updateWorkspace((state) => state.workspaceWindows.some(window => workspaceWindowKey(window) === key)
      ? { workspaceActiveKey: key, workspaceOpen: true, workspaceFocusRevision: state.workspaceFocusRevision + 1 } : {}),
    closeWorkspaceWindow: (key) => updateWorkspace((state) => {
      const index = state.workspaceWindows.findIndex(window => workspaceWindowKey(window) === key);
      if (index < 0) return {};
      const windows = state.workspaceWindows.filter(window => workspaceWindowKey(window) !== key);
      const next = windows[Math.min(index, windows.length - 1)];
      return {
        workspaceWindows: windows,
        workspaceActiveKey: state.workspaceActiveKey === key ? next ? workspaceWindowKey(next) : null : state.workspaceActiveKey,
        workspaceOpen: windows.length > 0 && state.workspaceOpen,
        workspaceExpanded: windows.length > 0 && state.workspaceExpanded,
        workspaceFocusRevision: state.workspaceFocusRevision + 1,
      };
    }),
    setWorkspacePreviewPosition: (ownerKey, key, position) => updateOwnedWindows(ownerKey, windows => windows.map(window => workspaceWindowKey(window) === key && (window.kind === "file" || window.kind === "sessionFile") ? { ...window, position } : window)),
    navigateWorkspaceWebpage: (ownerKey, key, url, title) => {
      const address = normalizeWorkspaceWebpageUrl(url);
      if (!address) throw new Error("Enter an HTTP or HTTPS address without credentials.");
      updateOwnedWindows(ownerKey, windows => windows.map(window => window.kind === "webpage" && workspaceWindowKey(window) === key ? { ...window, url: address, title } : window));
    },
    setWorkspaceOpen: (open) => updateWorkspace((state) => ({
      workspaceOpen: open && state.workspaceWindows.length > 0,
      ...(!open ? { workspaceExpanded: false } : {}),
    })),
    setWorkspaceExpanded: (expanded) => set((state) => ({ workspaceExpanded: expanded && state.workspaceOpen })),
    uiTheme: "light",
    locale: "en",
    sidebarOpen: true,
    activeProjectId: defaultProjects[0].id,
    setUiTheme: (uiTheme) => set(() => ({ uiTheme })),
    setLocale: (locale) => set(() => ({ locale })),
    setSidebarOpen: (sidebarOpen) => set(() => ({ sidebarOpen })),
    setActiveProjectId: (activeProjectId) => set(() => ({ activeProjectId })),
  };
}
