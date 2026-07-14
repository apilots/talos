// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom";

import "@/shared/i18n/setup";
import { ChatSessionPage } from "./ChatSessionPage";

const mocks = vi.hoisted(() => ({
  sendText: vi.fn(),
  sendTurn: vi.fn(),
  mutateModel: vi.fn(),
  mutateProject: vi.fn(),
  session: undefined as unknown,
  sessionHistoryError: undefined as Error | undefined,
  sessionHistoryIsSuccess: true,
  sessionQueryEnabled: undefined as boolean | undefined,
  composerProps: undefined as unknown,
}));

vi.mock("@/features/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/chat")>();

  return {
    ...actual,
    ChatComposer: (props: {
      disabled?: boolean;
      onModelChange?: (model: string) => void;
      onProjectChange: (projectId: string) => void;
      onReasoningLevelChange?: (level: "light" | "medium" | "high" | "extra_high") => void;
      onSend: () => Promise<void>;
      showRuntimeModeSelector?: boolean;
    }) => {
      mocks.composerProps = props;
      return (
        <div data-disabled={props.disabled ? "true" : "false"} data-testid="chat-composer">
          <button onClick={() => props.onModelChange?.("model-2")} type="button">
            Model
          </button>
          <button onClick={() => props.onProjectChange("project-1")} type="button">
            Project
          </button>
          <button onClick={() => props.onReasoningLevelChange?.("high")} type="button">
            Reasoning
          </button>
          <button onClick={() => void props.onSend()} type="button">
            Send
          </button>
        </div>
      );
    },
    useChatSession: () => ({
      blocks: [],
      draft: "",
      setDraft: vi.fn(),
      sendTurn: mocks.sendTurn,
      sendText: mocks.sendText,
      isStreaming: true,
      isCancelling: false,
      canRetryResponse: false,
      retryResponse: vi.fn(),
      retryStream: vi.fn(),
      showReconnectControls: false,
      stopResponse: vi.fn(),
      streamError: undefined,
      sessionHistoryQuery: {
        error: mocks.sessionHistoryError,
        isError: Boolean(mocks.sessionHistoryError),
        isLoading: false,
        isSuccess: mocks.sessionHistoryIsSuccess,
      },
    }),
    useModelsQuery: () => ({
      data: {
        defaultModelSelection: {
          modelRef: { provider: "mock", id: "model-1" },
          reasoningEffort: "medium",
        },
        models: [
          {
            id: "model-1",
            name: "Model 1",
            provider: "mock",
            reasoning: true,
            reasoning_effort: "medium",
          },
        ],
      },
    }),
    useProjectsView: () => ({
      activeProjectId: "__unbound_project__",
      projects: [
        {
          id: "project-1",
          name: "Project 1",
          directory: "/tmp/project-1",
        },
      ],
      setActiveProjectId: vi.fn(),
    }),
    useSessionQuery: (_sessionId: string, enabled?: boolean) => {
      mocks.sessionQueryEnabled = enabled;
      return { data: mocks.session };
    },
    useUpdateSessionModelMutation: () => ({ mutate: mocks.mutateModel }),
    useUpdateSessionProjectMutation: () => ({ mutate: mocks.mutateProject }),
  };
});

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      connection: {
        runtimeMode: "local",
      },
      setConnection: vi.fn(),
    }),
}));

function NavigateAwayButton() {
  const navigate = useNavigate();
  return (
    <button onClick={() => navigate("/settings")} type="button">
      Settings
    </button>
  );
}

describe("ChatSessionPage", () => {
  let resolveSend: () => void;
  let rejectSend: (error: Error) => void;

  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    mocks.session = {
      context: { session_id: "sess-1" },
      title: "Session 1",
      project_id: null,
      config: { llm: { model: "model-1" } },
      state: { message_count: 0 },
      modelSelection: {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: null,
      },
    };
    mocks.sessionHistoryError = undefined;
    mocks.sessionHistoryIsSuccess = true;
    mocks.composerProps = undefined;
    mocks.sessionQueryEnabled = undefined;
    mocks.sendText.mockImplementation(
      () =>
        new Promise<void>((resolve, reject) => {
          resolveSend = resolve;
          rejectSend = reject;
        }),
    );
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("keeps model and project changes local before the session exists", () => {
    mocks.session = undefined;

    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    fireEvent.click(screen.getByRole("button", { name: "Project" }));

    expect(mocks.mutateModel).not.toHaveBeenCalled();
    expect(mocks.mutateProject).not.toHaveBeenCalled();
  });

  it("hides runtime mode selector on the session detail composer", () => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect((mocks.composerProps as { showRuntimeModeSelector?: boolean }).showRuntimeModeSelector).toBe(false);
  });

  it("submits an explicit reasoning effort through model selection", () => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Reasoning" }));

    expect(mocks.mutateModel).toHaveBeenCalledWith({
      modelRef: { provider: "mock", id: "model-1" },
      reasoningEffort: "high",
    });
  });

  it("keeps draft sessions sendable after the initial prompt fails", async () => {
    mocks.session = undefined;
    mocks.sessionHistoryError = new Error("session not found");
    mocks.sessionHistoryIsSuccess = false;
    mocks.sendTurn.mockResolvedValue(undefined);

    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "hello" } }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(mocks.sendText).toHaveBeenCalledWith("hello", {
        modelSelection: undefined,
        project: undefined,
      });
    });
    expect(mocks.sessionQueryEnabled).toBe(false);
    expect(screen.queryByText("Session history failed to load.")).toBeNull();

    await act(async () => {
      rejectSend(new Error("first turn failed"));
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe("false");
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(mocks.sendTurn).toHaveBeenCalledWith({
      modelSelection: {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: "medium",
      },
      project: undefined,
      skills: undefined,
    });
  });

  it("does not navigate back to the session after leaving while the initial prompt is running", async () => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "hello" } }]}>
        <Routes>
          <Route
            element={
              <>
                <NavigateAwayButton />
                <ChatSessionPage />
              </>
            }
            path="/sessions/:sessionId"
          />
          <Route element={<div>Settings page</div>} path="/settings" />
        </Routes>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(mocks.sendText).toHaveBeenCalledWith("hello", {
        modelSelection: undefined,
        project: undefined,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByText("Settings page")).toBeTruthy();

    await act(async () => {
      resolveSend();
      await Promise.resolve();
    });

    expect(screen.getByText("Settings page")).toBeTruthy();
  });

  it("shows history errors instead of the empty ready state", () => {
    mocks.sessionHistoryError = new Error("history parse failed");
    mocks.sessionHistoryIsSuccess = false;

    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect(screen.getByText("Session history failed to load.")).toBeTruthy();
    expect(screen.getByText("history parse failed")).toBeTruthy();
    expect(screen.queryByText("This chat is ready.")).toBeNull();
  });

  it("shows the empty ready state after history loads successfully with no blocks", () => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect(screen.getByText("This chat is ready.")).toBeTruthy();
    expect(screen.queryByText("Session history failed to load.")).toBeNull();
  });
});
