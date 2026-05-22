// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom";

import "@/shared/i18n/setup";
import { ChatSessionPage } from "./ChatSessionPage";

const mocks = vi.hoisted(() => ({
  sendText: vi.fn(),
  mutate: vi.fn(),
}));

vi.mock("@/features/chat", () => ({
  ChatComposer: () => <div data-testid="chat-composer" />,
  useChatSession: () => ({
    blocks: [],
    draft: "",
    setDraft: vi.fn(),
    sendMessage: vi.fn(),
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
  useModelsQuery: () => ({ data: [{ id: "model-1", name: "Model 1" }] }),
  useProjectsQuery: () => ({ data: [] }),
  useSessionQuery: () => ({
    data: {
      context: { session_id: "sess-1" },
      title: "Session 1",
      project_id: null,
      config: { llm: { model: "model-1" } },
      state: { message_count: 0 },
    },
  }),
  useUpdateSessionModelMutation: () => ({ mutate: mocks.mutate }),
  useUpdateSessionProjectMutation: () => ({ mutate: mocks.mutate }),
}));

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: (selector: (state: Record<string, unknown>) => unknown) =>
    selector({
      activeProjectId: "__unbound_project__",
      connection: {
        runtimeMode: "cloud",
      },
      projects: [],
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

  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    mocks.sendText.mockImplementation(
      () =>
        new Promise<void>((resolve) => {
          resolveSend = resolve;
        }),
    );
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
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
      expect(mocks.sendText).toHaveBeenCalledWith("hello");
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
