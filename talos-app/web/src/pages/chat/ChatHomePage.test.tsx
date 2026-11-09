// @vitest-environment jsdom

import { act, cleanup, fireEvent, render as renderView, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Link, MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { createJSONStorage } from "zustand/middleware";

import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { createDefaultProjects } from "@/shared/types/domain";
import "@testing-library/jest-dom/vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { ChatHomePage } from "./ChatHomePage";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

let testProjects = createDefaultProjects();
const computer = vi.hoisted(() => ({ sleeping: false }));
const createProjectMutateAsync = vi.fn();
const setActiveProjectIdMock = vi.fn((projectId: string) => {
  useAppStore.setState({ activeProjectId: projectId });
});

vi.mock("@/features/chat", () => ({
  ChatComposer: ({
    activeProjectId,
    automationCommands,
    onPermissionModeChange,
    onSend,
    permissionMode,
    reasoningLevel,
    selectedModel,
  }: {
    activeProjectId: string;
    automationCommands?: Array<{ mode: string }>;
    onPermissionModeChange: (mode: "approveForMe") => void;
    onSend: (attachments: [], mode?: "goal" | "loop") => Promise<void> | void;
    permissionMode: string;
    reasoningLevel: string;
    selectedModel?: string;
  }) => (
    <div>
      <span data-testid="composer-project">{activeProjectId}</span>
      <span data-testid="composer-automation">{automationCommands?.map((command) => command.mode).join(", ")}</span>
      <span data-testid="composer-model">{selectedModel}</span>
      <span data-testid="composer-reasoning">{reasoningLevel}</span>
      <span data-testid="composer-permission">{permissionMode}</span>
      <button onClick={() => onPermissionModeChange("approveForMe")}>Approve for me</button>
      <button onClick={() => void onSend([])}>Send</button>
      <button onClick={() => void onSend([], "goal")}>Send Goal</button>
      <button onClick={() => void onSend([], "loop")}>Send Loop</button>
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
    projects: testProjects,
    setActiveProjectId: setActiveProjectIdMock,
  }),
}));

function LocationStateProbe() {
  const location = useLocation();
  return <pre data-testid="location-state">{JSON.stringify(location.state)}</pre>;
}

describe("ChatHomePage", () => {
  afterEach(() => {
    cleanup(); resetTalosClientFactory();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    computer.sleeping = false;
    setTalosClientFactory(() => ({ automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);

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
    testProjects = [
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
      ];
    useAppStore.setState({
      activeProjectId: "missing-project",
    });
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
        initialNodeId: "embedded",
        initialPrompt: "hello",
        initialPermissionMode: "approveForMe",
      });
    });
  });

  it("loads Add commands and submits from a cold Home with a sleeping Talo", async () => {
    computer.sleeping = true;
    const commandCatalog = vi.fn().mockResolvedValue({ data: [{ mode: "goal" }, { mode: "loop" }] });
    setTalosClientFactory(() => ({ automation: { commandCatalog } }) as unknown as TalosWebClient);
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "5m Check CI" } }]}><Routes>
      <Route path="/" element={<ChatHomePage />} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    await waitFor(() => expect(screen.getByTestId("composer-automation")).toHaveTextContent("goal, loop"));
    fireEvent.click(screen.getByRole("button", { name: "Send Loop" }));
    await waitFor(() => expect(JSON.parse(screen.getByTestId("location-state").textContent ?? "{}")).toMatchObject({
      initialNodeId: "talo", initialPrompt: "5m Check CI", initialSkillSelection: { name: "loop", path: "" },
    }));
  });

  it("does not navigate back when a protected-Skill lookup resolves after leaving Home", async () => {
    let resolve!: (value: { data: unknown[] }) => void;
    const list = vi.fn(() => new Promise<{ data: unknown[] }>((done) => { resolve = done; }));
    setTalosClientFactory(() => ({ skills: { list }, automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Original task" } }]}><Routes>
      <Route path="/" element={<><ChatHomePage /><Link to="/settings">Leave Home</Link></>} />
      <Route path="/settings" element={<div>Settings destination</div>} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: "Send Goal" }));
    await waitFor(() => expect(list).toHaveBeenCalledOnce());
    fireEvent.click(screen.getByRole("link", { name: "Leave Home" }));
    await act(async () => { resolve({ data: [{ name: "goal", scope: "system", enabled: true, read_only: true, path: "/real/goal/SKILL.md" }] }); });
    expect(screen.getByText("Settings destination")).toBeInTheDocument();
    expect(screen.queryByTestId("location-state")).toBeNull();
  });

  it.each(["goal", "loop"] as const)("preserves the selected %s and exact prompt through initial navigation", async (mode) => {
    const list = vi.fn().mockResolvedValue({ data: [
      { name: mode, scope: "project", path: "/project/SKILL.md", enabled: true, read_only: false },
      { name: mode, scope: "system", path: `/worker/${mode}/SKILL.md`, enabled: true, read_only: true },
    ] });
    setTalosClientFactory(() => ({ skills: { list }, automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Check original task" } }]}><Routes>
      <Route path="/" element={<ChatHomePage />} /><Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: mode === "goal" ? "Send Goal" : "Send Loop" }));
    await waitFor(() => expect(JSON.parse(screen.getByTestId("location-state").textContent ?? "{}" )).toMatchObject({
      initialNodeId: "embedded", initialPrompt: "Check original task", initialSkillSelection: { name: mode, path: `/worker/${mode}/SKILL.md` },
    }));
    expect(list).toHaveBeenCalledWith({ nodeId: "embedded", include_disabled: false });
  });

});

vi.mock("@/features/computers", () => ({ useComputers: () => computer.sleeping
  ? { selectedId: "talo", selected: { available: false, managedWorkerPhase: "sleeping" } }
  : { selectedId: "embedded", selected: { available: true } }, useProductScope: () => "test-scope", ComputerContext: () => null }));
