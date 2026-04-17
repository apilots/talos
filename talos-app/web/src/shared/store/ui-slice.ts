import { createDefaultProjects, getProjectRecordKey } from "@/shared/types/domain";
import type { ProjectRecord, ThreadFilter } from "@/shared/types/ui";

export type UiTheme = "dark" | "light";
export type Locale = "en" | "zh-CN";

export interface UiSlice {
  uiTheme: UiTheme;
  locale: Locale;
  sidebarOpen: boolean;
  threadFilter: ThreadFilter;
  activeProjectId: string;
  projects: ProjectRecord[];
  setUiTheme: (theme: UiTheme) => void;
  setLocale: (locale: Locale) => void;
  setSidebarOpen: (open: boolean) => void;
  setThreadFilter: (filter: ThreadFilter) => void;
  setActiveProjectId: (projectId: string) => void;
  addProject: (project: ProjectRecord) => void;
  mergeProjects: (projects: ProjectRecord[], preferredProjectId?: string) => void;
}

export type UiSliceSetter = (updater: (state: UiSlice) => Partial<UiSlice>) => void;

export const defaultProjects = createDefaultProjects();

export function normalizeProjects(projects?: ProjectRecord[]) {
  const normalized = new Map<string, ProjectRecord>();

  for (const project of defaultProjects) {
    normalized.set(getProjectRecordKey(project.root), project);
  }

  for (const project of projects ?? []) {
    const isLegacyDefaultWorkspace =
      project.origin === "default" &&
      getProjectRecordKey(project.root) !== getProjectRecordKey(defaultProjects[0].root);
    const isLegacyManagedWorkspace =
      project.root.type === "managed_workspace" &&
      (project.root.name === "talos" || project.root.name.startsWith("server-default"));
    const isLegacyWorkspacePath =
      project.root.type === "server_visible_path" &&
      project.root.path === "/srv/talos/workspace";
    if (isLegacyDefaultWorkspace || isLegacyManagedWorkspace || isLegacyWorkspacePath) {
      continue;
    }

    const key = getProjectRecordKey(project.root);
    const current = normalized.get(key);
    if (!current) {
      normalized.set(key, project);
      continue;
    }

    normalized.set(key, {
      ...project,
      name: current.origin === "default" ? current.name : project.name,
      origin: current.origin ?? project.origin,
      sessionCount: project.sessionCount ?? current.sessionCount,
      lastUsedAt: Math.max(project.lastUsedAt, current.lastUsedAt),
    });
  }

  return [...normalized.values()].sort((left, right) => right.lastUsedAt - left.lastUsedAt);
}

export function mergeById<T extends { id: string }>(defaults: T[], persisted?: T[]) {
  const merged = new Map<string, T>();
  defaults.forEach((item) => merged.set(item.id, item));
  persisted?.forEach((item) => merged.set(item.id, item));
  return [...merged.values()];
}

export function createUiSlice(set: UiSliceSetter): UiSlice {
  return {
    uiTheme: "light",
    locale: "en",
    sidebarOpen: true,
    threadFilter: "all",
    activeProjectId: defaultProjects[0].id,
    projects: defaultProjects,
    setUiTheme: (uiTheme) => set(() => ({ uiTheme })),
    setLocale: (locale) => set(() => ({ locale })),
    setSidebarOpen: (sidebarOpen) => set(() => ({ sidebarOpen })),
    setThreadFilter: (threadFilter) => set(() => ({ threadFilter })),
    setActiveProjectId: (activeProjectId) => set(() => ({ activeProjectId })),
    addProject: (project) =>
      set((state) => ({
        projects: normalizeProjects([project, ...state.projects]),
        activeProjectId: project.id,
      })),
    mergeProjects: (projects, preferredProjectId) =>
      set((state) => {
        const merged = normalizeProjects([...state.projects, ...projects]);
        const activeProjectExists = merged.some((project) => project.id === state.activeProjectId);
        const shouldFollowPreferred =
          !activeProjectExists || state.activeProjectId === defaultProjects[0].id;
        return {
          projects: merged,
          activeProjectId:
            shouldFollowPreferred && preferredProjectId
              ? preferredProjectId
              : activeProjectExists
                ? state.activeProjectId
                : merged[0]?.id ?? defaultProjects[0].id,
        };
      }),
  };
}
