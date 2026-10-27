import { createDefaultProjects, getProjectViewKey } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";
import { createUuid } from "@/shared/lib/uuid";
import { normalizeWorkspaceWebpageUrl, workspaceWindowKey, type WorkspaceWindow } from "./workspace-window";

export type UiTheme = "dark" | "light";
export type Locale = "en" | "zh-CN";
export const SIDEBAR_WIDTH = { initial: 296, min: 224, max: 480 } as const;
export const WORKSPACE_WIDTH = { initial: 480, min: 360, max: 1040 } as const;
export type ResizingPane = "sidebar" | "workspace" | null;

export interface UiSlice {
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
  openWorkspaceWindow: (window: Exclude<WorkspaceWindow, { kind: "webpage" }>) => void;
  createWorkspaceWebpage: () => void;
  selectWorkspaceWindow: (key: string) => void;
  closeWorkspaceWindow: (key: string) => void;
  navigateWorkspaceWebpage: (key: string, url: string) => void;
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
  return {
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
    openWorkspaceWindow: (window) => set((state) => {
      const key = workspaceWindowKey(window);
      return {
        workspaceWindows: state.workspaceWindows.some(entry => workspaceWindowKey(entry) === key)
          ? state.workspaceWindows : [...state.workspaceWindows, window],
        workspaceActiveKey: key,
        workspaceOpen: true,
      };
    }),
    createWorkspaceWebpage: () => set((state) => {
      const window: WorkspaceWindow = { kind: "webpage", id: createUuid(), url: "" };
      return { workspaceWindows: [...state.workspaceWindows, window], workspaceActiveKey: workspaceWindowKey(window), workspaceOpen: true };
    }),
    selectWorkspaceWindow: (key) => set((state) => state.workspaceWindows.some(window => workspaceWindowKey(window) === key)
      ? { workspaceActiveKey: key, workspaceOpen: true } : {}),
    closeWorkspaceWindow: (key) => set((state) => {
      const index = state.workspaceWindows.findIndex(window => workspaceWindowKey(window) === key);
      if (index < 0) return {};
      const windows = state.workspaceWindows.filter(window => workspaceWindowKey(window) !== key);
      const next = windows[Math.min(index, windows.length - 1)];
      return {
        workspaceWindows: windows,
        workspaceActiveKey: state.workspaceActiveKey === key ? next ? workspaceWindowKey(next) : null : state.workspaceActiveKey,
        workspaceOpen: windows.length > 0 && state.workspaceOpen,
        workspaceExpanded: windows.length > 0 && state.workspaceExpanded,
      };
    }),
    navigateWorkspaceWebpage: (key, url) => set((state) => {
      if (!state.workspaceWindows.some(window => window.kind === "webpage" && workspaceWindowKey(window) === key)) return {};
      const address = normalizeWorkspaceWebpageUrl(url);
      if (!address) throw new Error("Enter an HTTP or HTTPS address without credentials.");
      return { workspaceWindows: state.workspaceWindows.map(window => workspaceWindowKey(window) === key ? { ...window, url: address } : window) };
    }),
    setWorkspaceOpen: (open) => set((state) => ({
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
