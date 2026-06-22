import { createDefaultProjects, getProjectViewKey } from "@/shared/types/domain";
import type { ProjectView, ThreadFilter } from "@/shared/types/ui";

export type UiTheme = "dark" | "light";
export type Locale = "en" | "zh-CN";

export interface UiSlice {
  uiTheme: UiTheme;
  locale: Locale;
  sidebarOpen: boolean;
  threadFilter: ThreadFilter;
  activeProjectId: string;
  projects: ProjectView[];
  setUiTheme: (theme: UiTheme) => void;
  setLocale: (locale: Locale) => void;
  setSidebarOpen: (open: boolean) => void;
  setThreadFilter: (filter: ThreadFilter) => void;
  setActiveProjectId: (projectId: string) => void;
  addProject: (project: ProjectView) => void;
  mergeProjects: (projects: ProjectView[], preferredProjectId?: string) => void;
  replaceProjects: (projects: ProjectView[], preferredProjectId?: string) => void;
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

export function mergeById<T extends { id: string }>(defaults: T[], persisted?: T[]) {
  const merged = new Map<string, T>();
  defaults.forEach((item) => merged.set(item.id, item));
  persisted?.forEach((item) => merged.set(item.id, item));
  return [...merged.values()];
}

export function createUiSlice(set: UiSliceSetter): UiSlice {
  function resolveActiveProjectId(
    projects: ProjectView[],
    currentProjectId: string,
    preferredProjectId?: string,
  ) {
    const activeProjectExists = projects.some((project) => project.id === currentProjectId);
    const shouldFollowPreferred =
      !activeProjectExists || currentProjectId === defaultProjects[0].id;

    if (shouldFollowPreferred && preferredProjectId) {
      return preferredProjectId;
    }
    if (activeProjectExists) {
      return currentProjectId;
    }
    return projects[0]?.id ?? defaultProjects[0].id;
  }

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
        return {
          projects: merged,
          activeProjectId: resolveActiveProjectId(
            merged,
            state.activeProjectId,
            preferredProjectId,
          ),
        };
      }),
    replaceProjects: (projects, preferredProjectId) =>
      set((state) => {
        const normalized = normalizeProjects(projects);
        return {
          projects: normalized,
          activeProjectId: resolveActiveProjectId(
            normalized,
            state.activeProjectId,
            preferredProjectId,
          ),
        };
      }),
  };
}
