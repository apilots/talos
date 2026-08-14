// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
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
  ChatComposer: ({
    activeProjectId,
    onPermissionModeChange,
    onSend,
    permissionMode,
    reasoningLevel,
    selectedModel,
  }: {
    activeProjectId: string;
    onPermissionModeChange: (mode: "approveForMe") => void;
    onSend: () => void;
    permissionMode: string;
    reasoningLevel: string;
    selectedModel?: string;
  }) => (
    <div>
      <span data-testid="composer-project">{activeProjectId}</span>
      <span data-testid="composer-model">{selectedModel}</span>
      <span data-testid="composer-reasoning">{reasoningLevel}</span>
      <span data-testid="composer-permission">{permissionMode}</span>
      <button onClick={() => onPermissionModeChange("approveForMe")}>Approve for me</button>
      <button onClick={onSend}>Send</button>
    </div>
  ),
  useCreateProjectMutation: () => ({
    mutateAsync: createProjectMutateAsync,
  }),
  generateSessionId: () => "session-1",
  modelInfoForRef: (
    models: Array<{ provider: string; id: string }> | undefined,
    modelRef: { provider: string; id: string } | undefined,
  ) =>
    modelRef
      ? models?.find(
          (model) => model.provider === modelRef.provider && model.id === modelRef.id,
        )
      : undefined,
  supportedAttachmentKinds: () => [],
  modelRefForKey: (key: string) => {
    const separator = key.indexOf(":");
    return { provider: key.slice(0, separator), id: key.slice(separator + 1) };
  },
  modelRefKey: (modelRef: { provider: string; id: string }) =>
    `${modelRef.provider}:${modelRef.id}`,
  modelPickerOptions: (models: Array<{ provider: string; id: string; name: string }>) =>
    models.map((model) => ({
      id: `${model.provider}:${model.id}`,
      name: model.name,
      provider: model.provider,
    })),
  reasoningLevelForEffort: (effort: string | null) =>
    effort === "xhigh" ? "extra_high" : (effort ?? "none"),
  reasoningLevelForModel: () => "high",
  useModelsQuery: () => ({
    data: {
      defaultModelSelection: {
        modelRef: { provider: "openrouter", id: "tencent/hy3:free" },
        reasoningEffort: "high",
      },
      models: [
        { provider: "ollama", id: "llama-3.3", name: "Llama 3.3", reasoning: true },
        {
          provider: "openrouter",
          id: "tencent/hy3:free",
          name: "Tencent HY3 Free",
          reasoning: true,
        },
      ],
    },
  }),
  useProjectsView: () => ({
    activeProjectId: useAppStore.getState().activeProjectId,
    projects: useAppStore.getState().projects,
    setActiveProjectId: setActiveProjectIdMock,
  }),
}));

function LocationStateProbe() {
  const location = useLocation();
  return <pre data-testid="location-state">{JSON.stringify(location.state)}</pre>;
}

describe("ChatHomePage", () => {
  afterEach(() => {
    cleanup();
  });

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
    expect(screen.getByTestId("composer-model").textContent).toBe(
      "openrouter:tencent/hy3:free",
    );
    expect(screen.getByTestId("composer-reasoning").textContent).toBe("high");
  });

  it("carries the selected permission mode into a new session draft", async () => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "hello" } }]}>
        <Routes>
          <Route path="/" element={<ChatHomePage />} />
          <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
        </Routes>
      </MemoryRouter>,
    );

    expect(screen.getByTestId("composer-permission").textContent).toBe("fullAccess");
    fireEvent.click(screen.getByRole("button", { name: "Approve for me" }));
    expect(screen.getByTestId("composer-permission").textContent).toBe("approveForMe");

    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => {
      expect(JSON.parse(screen.getByTestId("location-state").textContent ?? "{}")).toMatchObject({
        initialPrompt: "hello",
        initialPermissionMode: "approveForMe",
      });
    });
  });
});
