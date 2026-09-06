// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom";

import "@/shared/i18n/setup";
import { ChatSessionPage } from "./ChatSessionPage";
import { getChatSessionPageMocks } from "./ChatSessionPage.test-support";

vi.mock("@/features/chat", async (importOriginal) => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createChatFeatureMock(importOriginal);
});
vi.mock("@/shared/store/app-store", async () => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createAppStoreMock();
});

const mocks = getChatSessionPageMocks();

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
      projectId: null,
      config: { llm: { model: "model-1" } },
      state: { message_count: 0 },
      modelSelection: {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: null,
      },
    };
    mocks.sessionHistoryError = undefined;
    mocks.sessionHistoryIsSuccess = true;
    mocks.isStreaming = false;
    mocks.composerProps = undefined;
    mocks.sessionQueryEnabled = undefined;
    mocks.sendText.mockImplementation(
      () =>
        new Promise<void>((resolve, reject) => {
          resolveSend = resolve;
          rejectSend = reject;
        }),
    );
    mocks.setDraft.mockReset();
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

  it("keeps permission changes local before the session exists", () => {
    mocks.session = undefined;
    mocks.sendTurn.mockResolvedValue(undefined);

    render(
      <MemoryRouter
        initialEntries={[{
          pathname: "/sessions/sess-1",
          state: { initialPermissionMode: "askForApproval" },
        }]}
      >
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect((mocks.composerProps as {
      permissionMode?: string;
      permissionModeDisabled?: boolean;
    })).toMatchObject({
      permissionMode: "askForApproval",
      permissionModeDisabled: false,
    });

    act(() => {
      (mocks.composerProps as {
        onPermissionModeChange: (mode: "approveForMe") => void;
      }).onPermissionModeChange("approveForMe");
    });
    expect((mocks.composerProps as { permissionMode?: string }).permissionMode).toBe("approveForMe");
    expect(mocks.mutatePermissionMode).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(mocks.sendTurn).toHaveBeenCalledWith(expect.objectContaining({
      userPermissionMode: "approveForMe",
    }));
  });

  it("requires confirmation before enabling full access", () => {
    mocks.session = {
      ...mocks.session as object,
      effectiveMode: "askForApproval",
    };
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    (mocks.composerProps as {
      onPermissionModeChange: (mode: "fullAccess") => void;
    }).onPermissionModeChange("fullAccess");
    expect(confirm).toHaveBeenCalledOnce();
    expect(mocks.mutatePermissionMode).not.toHaveBeenCalled();

    confirm.mockReturnValue(true);
    (mocks.composerProps as {
      onPermissionModeChange: (mode: "fullAccess") => void;
    }).onPermissionModeChange("fullAccess");
    expect(mocks.mutatePermissionMode).toHaveBeenCalledWith("fullAccess");
  });

  it("uses the persisted effective mode after the session loads", () => {
    mocks.session = undefined;
    const view = render(
      <MemoryRouter
        initialEntries={[{
          pathname: "/sessions/sess-1",
          state: { initialPermissionMode: "askForApproval" },
        }]}
      >
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect((mocks.composerProps as { permissionMode?: string }).permissionMode).toBe(
      "askForApproval",
    );

    mocks.session = {
      effectiveMode: "approveForMe",
      messageCount: 1,
      modelSelection: {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: null,
      },
      projectId: null,
      title: "Session 1",
    };
    view.rerender(
      <MemoryRouter
        initialEntries={[{
          pathname: "/sessions/sess-1",
          state: { initialPermissionMode: "askForApproval" },
        }]}
      >
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect((mocks.composerProps as { permissionMode?: string }).permissionMode).toBe(
      "approveForMe",
    );
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

    expect(mocks.mutateModel).toHaveBeenCalledWith(
      {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: "high",
      },
      expect.objectContaining({ onError: expect.any(Function), onSuccess: expect.any(Function) }),
    );
  });

  it("keeps draft sessions sendable after the initial prompt fails", async () => {
    mocks.session = undefined;
    mocks.sessionHistoryError = new Error("session not found");
    mocks.sessionHistoryIsSuccess = false;
    mocks.sendTurn.mockResolvedValue(undefined);

    render(
      <MemoryRouter initialEntries={[{
        pathname: "/sessions/sess-1",
        state: { initialPrompt: "hello", initialPermissionMode: "approveForMe" },
      }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(mocks.sendText).toHaveBeenCalledWith("hello", {
        modelSelection: undefined,
        userPermissionMode: "approveForMe",
        project: undefined,
      });
    });
    expect(screen.queryByText("Session history failed to load.")).toBeNull();

    await act(async () => {
      rejectSend(new Error("first turn failed"));
      await Promise.resolve();
    });

    await waitFor(() => expect(mocks.setDraft).toHaveBeenCalledWith("hello"));
    expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe("false");
    expect(mocks.sendText).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(mocks.sendTurn).toHaveBeenCalledWith({
      modelSelection: {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: "medium",
      },
      project: undefined,
      userPermissionMode: "approveForMe",
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
