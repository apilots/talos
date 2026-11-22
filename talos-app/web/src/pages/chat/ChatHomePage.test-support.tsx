import { cleanup, render as renderView } from "@testing-library/react";
import { afterEach, beforeEach, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createJSONStorage } from "zustand/middleware";
import type { ComponentProps, ReactElement } from "react";
import { useAppStore } from "@/shared/store/app-store";
import { createDefaultProjects } from "@/shared/types/domain";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

export function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

let testProjects = createDefaultProjects();
const composer = vi.hoisted(() => ({ real: false, nextSession: 0 }));
const startTurn = vi.hoisted(() => vi.fn());
const computer = vi.hoisted(() => ({ phase: undefined as "ready" | "sleeping" | undefined }));
const createProjectMutateAsync = vi.fn();
export const setActiveProjectIdMock = vi.fn((projectId: string) => {
  useAppStore.setState({ activeProjectId: projectId });
});

vi.mock("@/features/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/chat")>();
  return { ...actual,
  ChatComposer: (props: ComponentProps<typeof actual.ChatComposer>) => {
    if (composer.real) return <actual.ChatComposer {...props} />;
    const { activeProjectId, automationCommands, onPermissionModeChange, onSend,
      permissionMode, reasoningLevel, selectedModel } = props;
    return (
    <div>
      <span data-testid="composer-project">{activeProjectId}</span>
      <span data-testid="composer-automation">{automationCommands?.map((command) => command.mode).join(", ")}</span>
      <span data-testid="composer-model">{selectedModel}</span>
      <span data-testid="composer-reasoning">{reasoningLevel}</span>
      <span data-testid="composer-permission">{permissionMode}</span>
      <button onClick={() => onPermissionModeChange?.("approveForMe")}>Approve for me</button>
      <button onClick={() => void onSend([])}>Send</button>
      <button onClick={() => void onSend([], "goal")}>Send Goal</button>
      <button onClick={() => void onSend([], "loop")}>Send Loop</button>
    </div>
    );
  },
  useCreateProjectMutation: () => ({
    mutateAsync: createProjectMutateAsync,
  }),
  generateSessionId: () => `session-${++composer.nextSession}`,
  useModelsQuery: () => ({
    data: {
      defaultModelSelection: {
        modelRef: { provider: "openrouter", id: "tencent/hy3:free" },
        reasoningEffort: "high",
      },
      models: [
        { provider: "ollama", id: "llama-3.3", name: "Llama 3.3", input: ["text", "image"], reasoning: true, reasoning_effort: "high", selection_capabilities: { reasoning: "toggle", default_reasoning_effort: "high", default_fast: false } },
        {
          provider: "openrouter",
          id: "tencent/hy3:free",
          name: "Tencent HY3 Free",
          input: ["text", "image"],
          reasoning: true, reasoning_effort: "high",
          selection_capabilities: { reasoning: "toggle", default_reasoning_effort: "high", default_fast: false },
        },
      ],
    },
  }),
  useProjectsView: () => ({
    activeProjectId: useAppStore.getState().activeProjectId,
    projects: testProjects,
    setActiveProjectId: setActiveProjectIdMock,
  }),
};
});


afterEach(() => {
  cleanup(); resetTalosClientFactory(); vi.unstubAllGlobals();
});

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  vi.clearAllMocks();
  startTurn.mockReset().mockResolvedValue({ turnId: "turn-1", messageId: "message-1" });
  computer.phase = undefined;
  composer.real = false; composer.nextSession = 0;
  setTalosClientFactory(() => ({ turns: { start: startTurn }, automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);

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
    composerDrafts: {},
    composerRequests: {},
  });
});


vi.mock("@/features/computers", () => ({ useComputers: () => computer.phase
  ? { selectedId: "talo", selected: { available: computer.phase === "ready", managedWorkerPhase: undefined } }
  : { selectedId: "embedded", selected: { available: true } }, useManagedWorkerStatus: () => ({ status: { data: { instance: computer.phase ? { nodeId: "talo", phase: computer.phase } : null } } }), useProductScope: () => "test-scope", ComputerContext: () => null }));

export { composer, startTurn, computer };
