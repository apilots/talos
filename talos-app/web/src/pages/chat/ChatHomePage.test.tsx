// @vitest-environment jsdom

import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter } from "react-router-dom";
import { createJSONStorage } from "zustand/middleware";

import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { createDefaultProjects } from "@/shared/types/domain";
import { ChatHomePage } from "./ChatHomePage";

const createProjectMutateAsync = vi.fn();
const setActiveProjectIdMock = vi.fn((projectId: string) => {
  useAppStore.setState({ activeProjectId: projectId });
});

vi.mock("@/features/chat", () => ({
  ChatComposer: ({ activeProjectId }: { activeProjectId: string }) => (
    <div data-testid="composer-project">{activeProjectId}</div>
  ),
  useCreateProjectMutation: () => ({
    mutateAsync: createProjectMutateAsync,
  }),
  generateSessionId: () => "session-1",
  useModelsQuery: () => ({
    data: [{ id: "glm-5", name: "GLM-5" }],
  }),
  useProjectsView: () => ({
    activeProjectId: useAppStore.getState().activeProjectId,
    projects: useAppStore.getState().projects,
    setActiveProjectId: setActiveProjectIdMock,
  }),
}));

describe("ChatHomePage", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    const storage = new Map<string, string>();
    useAppStore.persist.setOptions({
      storage: createJSONStorage(() => ({
        getItem: (key: string) => storage.get(key) ?? null,
        setItem: (key: string, value: string) => {
          storage.set(key, value);
        },
        removeItem: (key: string) => {
          storage.delete(key);
        },
      })),
    });

    const [unboundProject] = createDefaultProjects();
    useAppStore.setState((state) => ({
      ...state,
      projects: [
        unboundProject,
        {
          id: "project-1",
          name: "alpha",
          directory: "/workspace/alpha",
          createdAt: 1,
          updatedAt: 2,
          origin: "server",
          sessionCount: 1,
        },
      ],
      activeProjectId: "missing-project",
    }));
  });

  it("restores the active project when persisted selection is stale", async () => {
    render(
      <MemoryRouter>
        <ChatHomePage />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(setActiveProjectIdMock).toHaveBeenCalledWith(createDefaultProjects()[0]?.id);
    });

    expect(screen.getByTestId("composer-project").textContent).toBe(createDefaultProjects()[0]?.id);
  });
});
