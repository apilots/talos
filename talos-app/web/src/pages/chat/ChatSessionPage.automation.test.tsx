// @vitest-environment jsdom

import { act, cleanup, fireEvent, render as renderView, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { Link, MemoryRouter, Route, Routes } from "react-router-dom";

import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
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
const apiMocks = vi.hoisted(() => ({
  phase: undefined as AppProtocol.ManagedWorkerPhase | undefined,
  reserved: undefined as string | undefined,
  list: vi.fn(),
}));
function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({
  automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) },
  skills: { list: apiMocks.list },
}) }));

function renderSessionPage(state?: { initialNodeId: string }) {
  return render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state }]}>
        <Routes>
          <Route element={<ChatSessionPage />} path="/sessions/:sessionId" />
        </Routes>
      </MemoryRouter>,
    );
}

describe("ChatSessionPage automation admission", () => {
  let resolveSend: () => void;
  let rejectSend: (error: Error) => void;

  beforeEach(() => {
    Element.prototype.scrollIntoView = vi.fn();
    resetChatSessionPageMocks();
    apiMocks.phase = undefined;
    apiMocks.reserved = undefined;
    apiMocks.list.mockReset().mockResolvedValue({ data: ["goal", "loop"].map((name) => ({ name, scope: "system", enabled: true, read_only: true, path: `/actual-worker/${name}/SKILL.md` })) });
    mocks.sendText.mockImplementation(() => new Promise<void>((resolve, reject) => {
      resolveSend = resolve;
      rejectSend = reject;
    }));
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("does not revive a consumed initial retry mode when returning to the same Session", async () => {
    mocks.realComposer = true; mocks.session = undefined; mocks.sessionError = sessionMissing(); mocks.draft = "Raw task";
    render(<MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "Raw task", initialNodeId: "embedded", initialSkillSelection: { name: "goal", path: "/actual-worker/goal/SKILL.md" } } }]}>
      <Link to="/sessions/sess-2">Other session</Link><Link to="/sessions/sess-1">Original session</Link>
      <Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes>
    </MemoryRouter>);
    await waitFor(() => expect(mocks.sendText).toHaveBeenCalledOnce());
    await act(async () => { rejectSend(new Error("Admission rejected")); });
    await waitFor(() => expect(screen.getByTestId("automation-mode-chip").textContent).toContain("Goal"));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(mocks.sendTurn).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).toBeNull());
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("link", { name: "Other session" }));
    fireEvent.click(screen.getByRole("link", { name: "Original session" }));
    expect(screen.queryByTestId("automation-mode-chip")).toBeNull();
  });

  it("rejects a retired Computer lookup instead of acknowledging an unsent message", async () => {
    mocks.draft = "Raw task";
    let release!: (value: { data: unknown[] }) => void;
    apiMocks.list.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const view = renderSessionPage();
    const props = mocks.composerProps as { onSend: (attachments: [], mode?: "goal" | "loop") => Promise<void> };
    const pending = props.onSend([], "goal");
    const observed = pending.then(() => "accepted", (error: unknown) => error instanceof DOMException ? error.name : "other-error");
    await waitFor(() => expect(apiMocks.list).toHaveBeenCalledOnce());
    mocks.session = { ...mocks.session as object, nodeId: "other-computer" };
    view.rerender(<MemoryRouter initialEntries={["/sessions/sess-1"]}><Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes></MemoryRouter>);
    await act(async () => { release({ data: [{ name: "goal", scope: "system", enabled: true, read_only: true, path: "/actual-worker/goal/SKILL.md" }] }); });
    expect(await observed).toBe("AbortError");
    expect(mocks.sendTurn).not.toHaveBeenCalled();
  });

  it.each(["goal", "loop"] as const)("attaches a protected %s selection to a new Session's first manual submission", async (mode) => {
    mocks.session = undefined;
    mocks.draft = "5m Original task";
    mocks.sessionError = sessionMissing();
    renderSessionPage({ initialNodeId: "embedded" });
    const props = mocks.composerProps as { onSend: (attachments: [], mode?: "goal" | "loop") => Promise<void> };
    await act(async () => { await props.onSend([], mode); });
    expect(mocks.sendTurn).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ nodeId: "embedded", skillSelection: { name: mode, path: `/actual-worker/${mode}/SKILL.md` } }));
  });

  it.each(["ready", "sleeping"] as const)("sends a Goal from a %s personal Talo deep link through Server admission", async (phase) => {
    mocks.session = { ...mocks.session as object, nodeId: "personal-talo" };
    apiMocks.phase = phase;
    mocks.draft = "Inspect CI";
    renderSessionPage();
    const props = mocks.composerProps as { onSend: (attachments: [], mode: "goal") => Promise<void> };
    await act(async () => { await props.onSend([], "goal"); });
    expect(mocks.sendTurn).toHaveBeenCalledExactlyOnceWith({ skillSelection: { name: "goal", path: "" } });
    expect(apiMocks.list).not.toHaveBeenCalled();
  });

  it("starts the exact sleeping conversation reserved by Server on a direct link", async () => {
    mocks.session = undefined;
    mocks.sessionError = sessionMissing();
    apiMocks.reserved = "sess-1";
    apiMocks.phase = "sleeping";
    mocks.draft = "5m Check CI";
    renderSessionPage();
    const props = mocks.composerProps as { onSend: (attachments: [], mode: "loop") => Promise<void> };
    await act(async () => { await props.onSend([], "loop"); });
    expect(mocks.sendTurn).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      nodeId: "personal-talo", skillSelection: { name: "loop", path: "" },
    }));
    expect(apiMocks.list).not.toHaveBeenCalled();
  });

  it("resumes a deep-linked conversation when its metadata becomes available", async () => {
    const session = mocks.session;
    mocks.session = undefined;
    mocks.sessionError = new TalosApiError({ code: AppProtocol.ErrorCode.ServiceUnavailable, message: "Session unavailable.", localization: null });
    mocks.draft = "Resume this conversation";
    mocks.realComposer = true;
    const view = renderSessionPage();
    expect((screen.getByRole("button", { name: "Send" }) as HTMLButtonElement).disabled).toBe(true);

    mocks.session = session;
    mocks.sessionError = undefined;
    view.rerender(<MemoryRouter initialEntries={["/sessions/sess-1"]}><Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes></MemoryRouter>);
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(mocks.sendTurn).toHaveBeenCalledOnce());
  });

  it("waits for initial admission before querying the newly created session", async () => {
    mocks.session = undefined;
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialNodeId: "embedded", initialPrompt: "hello" } }]}>
        <Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes>
      </MemoryRouter>,
    );
    await waitFor(() => expect(mocks.sendText).toHaveBeenCalled());
    expect(mocks.sessionQueryEnabled).toBe(false);
    await act(async () => { resolveSend(); });
    await waitFor(() => expect(mocks.sessionQueryEnabled).toBe(true));
  });

  it.each(["success", "failure"])("clears pending admission after navigating away and returning (%s)", async (outcome) => {
    mocks.session = undefined;
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialNodeId: "embedded", initialPrompt: "hello" } }]}>
        <Link to="/sessions/sess-2">Other session</Link>
        <Link to="/sessions/sess-1">Original session</Link>
        <Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes>
      </MemoryRouter>,
    );
    await waitFor(() => expect(mocks.sendText).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("link", { name: "Other session" }));
    expect(mocks.sessionQueryEnabled).toBe(true);
    await act(async () => {
      if (outcome === "success") resolveSend();
      else rejectSend(new Error("Admission failed."));
    });
    fireEvent.click(screen.getByRole("link", { name: "Original session" }));
    expect(mocks.sessionQueryEnabled).toBe(true);
  });


});
function sessionMissing() {
  return new TalosApiError({ code: AppProtocol.ErrorCode.ResourceNotFound, message: "Session not found.", localization: null });
}

vi.mock("@/features/computers", () => ({
  useProductScope: () => "test-scope",
  useComputer: (nodeId?: string) => ({ data: nodeId === "personal-talo"
    ? { available: apiMocks.phase === "ready", managedWorkerPhase: apiMocks.phase }
    : nodeId ? { available: true } : undefined }),
  useComputers: () => ({ selectedId: "embedded" }),
  useManagedWorkerStatus: () => ({ status: { data: { instance: {
    phase: apiMocks.phase, nodeId: "personal-talo", conversationSessionId: apiMocks.reserved,
  } } } }),
}));
