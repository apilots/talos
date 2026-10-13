import { createDefaultProjects, getProjectViewKey } from "@/shared/types/domain";
import type { ProjectView } from "@/shared/types/ui";

export type UiTheme = "dark" | "light";
export type Locale = "en" | "zh-CN";

export interface UiSlice {
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
