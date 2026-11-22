// @vitest-environment jsdom

import { act, cleanup, fireEvent, render as renderView, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryRouter, Route, Routes } from "react-router-dom";

import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { TalosApiError } from "@talos/sdk-browser";
import { ChatSessionPage } from "./ChatSessionPage";
import { getChatSessionPageMocks, resetChatSessionPageMocks } from "./ChatSessionPage.test-support";

vi.mock("@/features/automation/useSessionAutomation", () => ({
  useSessionAutomation: () => ({ tasks: [], commands: [], execute: vi.fn(), refresh: vi.fn(), command: vi.fn(), setEditing: vi.fn(), pending: false }),
}));

vi.mock("@/features/chat", async (importOriginal) => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createChatFeatureMock(importOriginal);
});
vi.mock("@/shared/store/app-store", async () => {
  const support = await import("./ChatSessionPage.test-support");
  return support.createAppStoreMock();
});

const mocks = getChatSessionPageMocks();
const apiMocks = vi.hoisted(() => ({ node: "embedded", reserved: false, list: vi.fn(), files: vi.fn(), draftFiles: vi.fn() }));
function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

vi.mock("@/shared/api/client", () => ({ getTalosClient: (owner?: { sessionId?: string }) => ({
  automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) },
  skills: { list: apiMocks.list },
  files: { list: owner?.sessionId === "sess-1" ? apiMocks.files : apiMocks.draftFiles },
}) }));

function renderSessionPage(reserved = false) {
  apiMocks.reserved = reserved;
  if (reserved) mocks.sessionError = new TalosApiError({ code: "resourceNotFound", message: "Not materialized.", localization: null });
  return render(
      <MemoryRouter initialEntries={["/sessions/sess-1"]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );
}

describe("ChatSessionPage", () => {

  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    resetChatSessionPageMocks();
    apiMocks.node = "embedded";
    apiMocks.reserved = false;
    apiMocks.list.mockReset().mockResolvedValue({ data: ["goal", "loop"].map((name) => ({ name, scope: "system", enabled: true, read_only: true, path: `/actual-worker/${name}/SKILL.md` })) });
    mocks.setDraft.mockReset();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it.each(["saved", "unsaved"] as const)("selects attachments for a %s session reached via its Server conversation URL", async (kind) => {
    if (kind === "unsaved") mocks.session = undefined;
    mocks.realComposer = true;
    apiMocks.files.mockReset().mockResolvedValue({ data: [], nextCursor: null });
    apiMocks.draftFiles.mockReset().mockResolvedValue({ data: [], nextCursor: null });
    renderSessionPage(kind === "unsaved");

    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose an existing file" }));

    await waitFor(() => expect(apiMocks.files).toHaveBeenCalledWith({ limit: 100 }));
  });

  it("keeps model and project changes local before the session exists", () => {
    mocks.session = undefined;

    renderSessionPage(true);

    expect(screen.getByTestId("chat-composer").getAttribute("data-disabled")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Model" }));
    fireEvent.click(screen.getByRole("button", { name: "Project" }));

    expect(mocks.mutateModel).not.toHaveBeenCalled();
  });

  it("keeps permission changes local before the session exists", async () => {
    mocks.session = undefined;
    mocks.sendTurn.mockResolvedValue(undefined);
    apiMocks.reserved = true;
    mocks.sessionError = new TalosApiError({ code: "resourceNotFound", message: "Not materialized.", localization: null });

    render(
      <MemoryRouter
        initialEntries={["/sessions/sess-1"]}
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
      permissionMode: "fullAccess",
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
    await waitFor(() => expect(mocks.sendTurn).toHaveBeenCalledWith(expect.objectContaining({
      userPermissionMode: "approveForMe",
    })));
  });

  it("requires confirmation before enabling full access", () => {
    mocks.session = {
      ...mocks.session as object,
      effectiveMode: "askForApproval",
    };
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    renderSessionPage();

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
        initialEntries={["/sessions/sess-1"]}
      >
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );

    expect((mocks.composerProps as { permissionMode?: string }).permissionMode).toBe(
      "fullAccess",
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
        initialEntries={["/sessions/sess-1"]}
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
    renderSessionPage();

    fireEvent.click(screen.getByRole("button", { name: "Reasoning" }));

    expect(mocks.mutateModel).toHaveBeenCalledWith(
      {
        modelRef: { provider: "mock", id: "model-1" },
        reasoningEffort: "high",
      },
      expect.objectContaining({ onError: expect.any(Function), onSuccess: expect.any(Function) }),
    );
  });

  it("shows history errors instead of the empty ready state", () => {
    mocks.sessionHistoryError = new Error("history parse failed");
    mocks.sessionHistoryIsSuccess = false;

    renderSessionPage();

    expect(screen.getByText("Session history failed to load.")).toBeTruthy();
    expect(screen.getByText("history parse failed")).toBeTruthy();
    expect(screen.queryByText("This chat is ready.")).toBeNull();
  });

  it("shows the empty ready state after history loads successfully with no blocks", () => {
    renderSessionPage();

    expect(screen.getByText("This chat is ready.")).toBeTruthy();
    expect(screen.queryByText("Session history failed to load.")).toBeNull();
  });
});

vi.mock("@/features/computers", () => ({ useManagedWorkerStatus: () => ({ status: { data: { instance: apiMocks.reserved ? { nodeId: "embedded", conversationSessionId: "sess-1", phase: "ready" } : undefined } } }), useComputers: () => ({ selectedId: apiMocks.node, select: vi.fn() }), useProductScope: () => "test-scope", useComputer: () => ({ data: { available: true } }) }));
