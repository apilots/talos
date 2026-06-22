import { describe, expect, it } from "vitest";

import {
  createUiSlice,
  defaultProjects,
  type UiSlice,
} from "./ui-slice";
import type { ProjectView } from "@/shared/types/ui";

describe("ui slice project state", () => {
  it("replaces stale server projects when a different user loads projects", () => {
    let state = {} as UiSlice;
    const set = (updater: (current: UiSlice) => Partial<UiSlice>) => {
      state = { ...state, ...updater(state) };
    };

    state = createUiSlice(set);

    const oldUserProject: ProjectView = {
      id: "project-old",
      name: "legacy",
      directory: "/workspace/legacy",
      createdAt: 1,
      updatedAt: 10,
      origin: "server",
      sessionCount: 1,
    };
    const currentUserProject: ProjectView = {
      id: "project-current",
      name: "claude-code",
      directory: "/workspace/claude-code",
      createdAt: 2,
      updatedAt: 20,
      origin: "server",
      sessionCount: 3,
    };

    state.mergeProjects([oldUserProject], oldUserProject.id);
    state.replaceProjects([currentUserProject], currentUserProject.id);

    expect(state.projects.map((project) => project.id)).toEqual([
      defaultProjects[0].id,
      currentUserProject.id,
    ]);
    expect(state.activeProjectId).toBe(currentUserProject.id);
  });
});
