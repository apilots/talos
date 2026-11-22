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
import type { ComponentProps, ReactElement } from "react";
import { ChatHomePage } from "./ChatHomePage";
import { createBrowserTalosClient } from "@talos/sdk-browser";
import { setTalosClientFactory, resetTalosClientFactory, type TalosWebClient } from "@/shared/api/client";

function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

let testProjects = createDefaultProjects();
const composer = vi.hoisted(() => ({ real: false }));
const startTurn = vi.hoisted(() => vi.fn());
const computer = vi.hoisted(() => ({ phase: undefined as "ready" | "sleeping" | undefined }));
const createProjectMutateAsync = vi.fn();
const setActiveProjectIdMock = vi.fn((projectId: string) => {
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
  generateSessionId: () => "session-1",
  useModelsQuery: () => ({
    data: {
      defaultModelSelection: {
        modelRef: { provider: "openrouter", id: "tencent/hy3:free" },
        reasoningEffort: "high",
      },
      models: [
        { provider: "ollama", id: "llama-3.3", name: "Llama 3.3", reasoning: true, selection_capabilities: { reasoning: "toggle", default_reasoning_effort: "high", default_fast: false } },
        {
          provider: "openrouter",
          id: "tencent/hy3:free",
          name: "Tencent HY3 Free",
          input: ["text", "image"],
          reasoning: true,
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
    startTurn.mockReset().mockResolvedValue({ turnId: "turn-1", messageId: "message-1" });
    computer.phase = undefined;
    composer.real = false;
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

  it("admits the first Turn with its selected permission mode before navigating", async () => {
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
      expect(startTurn).toHaveBeenCalledWith(expect.objectContaining({
        nodeId: "embedded", sessionId: "session-1", userPermissionMode: "approveForMe",
        input: { content: [{ type: "text", text: "hello" }] },
      }));
      expect(screen.getByTestId("location-state")).toHaveTextContent("null");
    });
  });

  it.each(["ready", "sleeping"] as const)("submits Add commands from Home through a %s Talo", async (phase) => {
    computer.phase = phase;
    const commandCatalog = vi.fn().mockResolvedValue({ data: [{ mode: "goal" }, { mode: "loop" }] });
    setTalosClientFactory(() => ({ turns: { start: startTurn }, automation: { commandCatalog } }) as unknown as TalosWebClient);
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "5m Check CI" } }]}><Routes>
      <Route path="/" element={<ChatHomePage />} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    await waitFor(() => expect(screen.getByTestId("composer-automation")).toHaveTextContent("goal, loop"));
    fireEvent.click(screen.getByRole("button", { name: "Send Loop" }));
    await waitFor(() => expect(startTurn).toHaveBeenCalledWith(expect.objectContaining({
      nodeId: "talo", input: { content: [{ type: "skillSelection", name: "loop", path: "" }, { type: "text", text: "5m Check CI" }] },
    })));
  });

  it("does not navigate back when a protected-Skill lookup resolves after leaving Home", async () => {
    let resolve!: (value: { data: unknown[] }) => void;
    const list = vi.fn(() => new Promise<{ data: unknown[] }>((done) => { resolve = done; }));
    setTalosClientFactory(() => ({ skills: { list }, turns: { start: startTurn }, automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
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

  it.each(["goal", "loop"] as const)("admits the selected %s and exact prompt through the existing Turn API", async (mode) => {
    const list = vi.fn().mockResolvedValue({ data: [
      { name: mode, scope: "project", path: "/project/SKILL.md", enabled: true, read_only: false },
      { name: mode, scope: "system", path: `/worker/${mode}/SKILL.md`, enabled: true, read_only: true },
    ] });
    setTalosClientFactory(() => ({ skills: { list }, turns: { start: startTurn }, automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) } }) as unknown as TalosWebClient);
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Check original task" } }]}><Routes>
      <Route path="/" element={<ChatHomePage />} /><Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: mode === "goal" ? "Send Goal" : "Send Loop" }));
    await waitFor(() => expect(startTurn).toHaveBeenCalledWith(expect.objectContaining({
      nodeId: "embedded", input: { content: [{ type: "skillSelection", name: mode, path: `/worker/${mode}/SKILL.md` }, { type: "text", text: "Check original task" }] },
    })));
    expect(list).toHaveBeenCalledWith({ nodeId: "embedded", include_disabled: false });
  });

  it("keeps the original input, attachment and Loop on rejected admission, then navigates after one manual retry", async () => {
    composer.real = true;
    let accept!: (value: { turnId: string; messageId: string }) => void;
    startTurn.mockRejectedValueOnce(new Error("Server admission unavailable"))
      .mockImplementationOnce(() => new Promise(resolve => { accept = resolve; }));
    const file = { fileId: "image-1", originalFilename: "diagram.png", mimeType: "image/png",
      sizeBytes: 8, scope: { type: "personal" as const }, tags: [], metadata: {}, createdAt: 1, updatedAt: 1 };
    setTalosClientFactory(() => ({
      turns: { start: startTurn },
      files: { upload: vi.fn().mockResolvedValue({ file }) },
      skills: { list: vi.fn().mockResolvedValue({ data: [{ name: "loop", scope: "system", path: "/worker/loop/SKILL.md", enabled: true, read_only: true }] }) },
      automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [{ mode: "loop", command: "/loop", description: "Loop" }] }) },
    }) as unknown as TalosWebClient);
    const view = render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "5m Check the original task" } }]}>
      <Routes><Route path="/" element={<ChatHomePage />} /><Route path="/sessions/:sessionId" element={<LocationStateProbe />} /></Routes>
    </MemoryRouter>);
    const input = screen.getByRole("textbox");
    fireEvent.change(view.container.querySelector('input[type="file"]')!, {
      target: { files: [new File(["image"], "diagram.png", { type: "image/png" })] },
    });
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove diagram.png" })).toBeVisible());
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(await screen.findByRole("button", { name: "Loop" }));
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(screen.getByText("Server admission unavailable")).toBeVisible());
    expect(input).toHaveValue("5m Check the original task");
    expect(screen.getByRole("button", { name: "Remove diagram.png" })).toBeVisible();
    expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent("Loop");
    expect(screen.queryByTestId("location-state")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(startTurn).toHaveBeenCalledTimes(2));
    expect(screen.queryByTestId("location-state")).toBeNull();
    expect(startTurn.mock.calls[1][0]).toEqual(startTurn.mock.calls[0][0]);
    expect(startTurn.mock.calls[1][0]).toMatchObject({ sessionId: "session-1",
      nodeId: "embedded", input: { content: [
        { type: "skillSelection", name: "loop", path: "/worker/loop/SKILL.md" },
        { type: "text", text: "5m Check the original task" }, { type: "file", fileId: "image-1" },
      ] } });
    await act(async () => { accept({ turnId: "turn-1", messageId: "message-1" }); });
    await waitFor(() => expect(screen.getByTestId("location-state")).toHaveTextContent("null"));
    expect(startTurn).toHaveBeenCalledTimes(2);
  });

  it.each([false, true])("keeps SDK retry identity after a lost ACK, and changes it only for edited input (edited=%s)", async (edited) => {
    composer.real = true;
    const accepted = new Map<string, string>();
    const keys: string[] = [];
    const request = vi.fn<typeof fetch>(async (_url, init) => {
      if (!String(_url).endsWith("/v1/turns/start")) return Response.json({ status: "success", data: { data: [] } });
      const key = new Headers(init?.headers).get("idempotency-key")!;
      const body = String(init?.body);
      keys.push(key);
      if (accepted.has(key)) expect(body).toBe(accepted.get(key));
      accepted.set(key, body);
      if (keys.length === 1) throw new TypeError("Server response lost");
      return Response.json({ status: "success", data: { turnId: "turn-1", messageId: "message-1" } });
    });
    setTalosClientFactory(() => createBrowserTalosClient({ baseUrl: "https://server.example", fetch: request }));
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Original task" } }]}><Routes>
      <Route path="/" element={<ChatHomePage />} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("alert");
    expect(screen.queryByTestId("location-state")).toBeNull();
    if (edited) fireEvent.change(screen.getByRole("textbox"), { target: { value: "Edited task" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByTestId("location-state");
    expect(keys).toHaveLength(2);
    expect(accepted.size).toBe(edited ? 2 : 1);
  });

  it("keeps pending input until acknowledgement and respects navigation away during admission", async () => {
    let accept!: (value: { turnId: string; messageId: string }) => void;
    startTurn.mockImplementationOnce(() => new Promise(resolve => { accept = resolve; }));
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Original task" } }]}>
      <Routes><Route path="/" element={<><ChatHomePage /><Link to="/settings">Leave Home</Link></>} />
        <Route path="/settings" element={<div>Settings destination</div>} />
        <Route path="/sessions/:sessionId" element={<LocationStateProbe />} /></Routes>
    </MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(startTurn).toHaveBeenCalledOnce());
    const key = JSON.stringify(["test-scope", "embedded", "home"]);
    expect(useAppStore.getState().composerDrafts[key]).toBe("Original task");
    expect(screen.queryByTestId("location-state")).toBeNull();
    fireEvent.click(screen.getByRole("link", { name: "Leave Home" }));
    await act(async () => { accept({ turnId: "turn-1", messageId: "message-1" }); });
    expect(screen.getByText("Settings destination")).toBeVisible();
    expect(useAppStore.getState().composerDrafts[key]).toBeUndefined();
  });

});

vi.mock("@/features/computers", () => ({ useComputers: () => computer.phase
  ? { selectedId: "talo", selected: { available: computer.phase === "ready", managedWorkerPhase: undefined } }
  : { selectedId: "embedded", selected: { available: true } }, useManagedWorkerStatus: () => ({ status: { data: { instance: computer.phase ? { nodeId: "talo", phase: computer.phase } : null } } }), useProductScope: () => "test-scope", ComputerContext: () => null }));
