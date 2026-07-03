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
  composerProps: undefined as unknown,
}));

vi.mock("@/features/chat", () => ({
  ChatComposer: (props: {
    disabled?: boolean;
    onModelChange?: (model: string) => void;
    onProjectChange: (projectId: string) => void;
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
  }),
  useModelsQuery: () => ({ data: [{ id: "model-1", name: "Model 1", provider: "mock" }] }),
  useProjectsQuery: () => ({ data: [] }),
  useSessionQuery: () => ({
    data: mocks.session,
  }),
  useUpdateSessionModelMutation: () => ({ mutate: mocks.mutateModel }),
  useUpdateSessionProjectMutation: () => ({ mutate: mocks.mutateProject }),
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      activeProjectId: "__unbound_project__",
      connection: {
        runtimeMode: "local",
      },
      projects: [
        {
          id: "project-1",
          name: "Project 1",
          directory: "/tmp/project-1",
        },
      ],
      setActiveProjectId: vi.fn(),
      setConnection: vi.fn(),
    }),
}));

function NavigateAwayButton() {
  const navigate = useNavigate();
  return (
    <button onClick={() => navigate("/projects")} type="button">
      Projects
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
    };
    mocks.composerProps = undefined;
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

  it("keeps draft sessions sendable after the initial prompt fails", async () => {
    mocks.session = undefined;
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
        modelRef: undefined,
        project: undefined,
      });
    });

    await act(async () => {
      rejectSend(new Error("first turn failed"));
      await Promise.resolve();
    });

    await waitFor(() => {
      expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe("false");
    });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(mocks.sendTurn).toHaveBeenCalledWith({
      modelRef: { provider: "mock", id: "model-1" },
      project: undefined,
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
          <Route element={<div>Projects page</div>} path="/projects" />
        </Routes>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(mocks.sendText).toHaveBeenCalledWith("hello", {
        modelRef: undefined,
        project: undefined,
      });
    });

    fireEvent.click(screen.getByRole("button", { name: "Projects" }));
    expect(screen.getByText("Projects page")).toBeTruthy();

    await act(async () => {
      resolveSend();
      await Promise.resolve();
    });

    expect(screen.getByText("Projects page")).toBeTruthy();
  });
});
