// @vitest-environment jsdom

import { act, cleanup, fireEvent, render as renderView, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
const apiMocks = vi.hoisted(() => ({ node: "embedded", list: vi.fn() }));
function render(ui: ReactElement) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return renderView(ui, { wrapper: ({ children }) => <QueryClientProvider client={client}>{children}</QueryClientProvider> });
}

vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({
  automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [] }) },
  skills: { list: apiMocks.list },
}) }));

function renderSessionPage() {
  return render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1" }]}>
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
    apiMocks.node = "embedded";
    apiMocks.list.mockReset().mockResolvedValue({ data: ["goal", "loop"].map((name) => ({ name, scope: "system", enabled: true, read_only: true, path: `/actual-worker/${name}/SKILL.md` })) });
    mocks.sendText.mockImplementation(() => new Promise<void>((resolve, reject) => {
      resolveSend = resolve;
      rejectSend = reject;
    }));
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("does not revive a consumed initial retry mode when returning to the same Session", async () => {
    mocks.realComposer = true; mocks.session = undefined; mocks.draft = "Raw task";
    render(<MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "Raw task", initialSkillSelection: { name: "goal", path: "/actual-worker/goal/SKILL.md" } } }]}>
      <Link to="/sessions/sess-2">Other session</Link><Link to="/sessions/sess-1">Original session</Link>
      <Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes>
    </MemoryRouter>);
    await waitFor(() => expect(mocks.sendText).toHaveBeenCalledOnce());
    await act(async () => { rejectSend(new Error("Admission rejected")); });
    await waitFor(() => expect(screen.getByTestId("automation-mode-chip").textContent).toContain("Goal"));
    fireEvent.keyDown(screen.getByRole("textbox"), { key: "Enter" });
    await waitFor(() => expect(mocks.sendTurn).toHaveBeenCalledOnce());
    await waitFor(() => expect(screen.queryByTestId("automation-mode-chip")).toBeNull());
    fireEvent.click(screen.getByRole("link", { name: "Other session" }));
    fireEvent.click(screen.getByRole("link", { name: "Original session" }));
    expect(screen.queryByTestId("automation-mode-chip")).toBeNull();
  });

  it("rejects a retired Computer lookup instead of acknowledging an unsent message", async () => {
    mocks.session = undefined; mocks.draft = "Raw task";
    let release!: (value: { data: unknown[] }) => void;
    apiMocks.list.mockImplementationOnce(() => new Promise((resolve) => { release = resolve; }));
    const view = renderSessionPage();
    const props = mocks.composerProps as { onSend: (attachments: [], mode?: "goal" | "loop") => Promise<void> };
    const pending = props.onSend([], "goal");
    const observed = pending.then(() => "accepted", (error: unknown) => error instanceof DOMException ? error.name : "other-error");
    await waitFor(() => expect(apiMocks.list).toHaveBeenCalledOnce());
    apiMocks.node = "other-computer";
    view.rerender(<MemoryRouter initialEntries={["/sessions/sess-1"]}><Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes></MemoryRouter>);
    await act(async () => { release({ data: [{ name: "goal", scope: "system", enabled: true, read_only: true, path: "/actual-worker/goal/SKILL.md" }] }); });
    expect(await observed).toBe("AbortError");
    expect(mocks.sendTurn).not.toHaveBeenCalled();
  });

  it.each(["goal", "loop"] as const)("attaches a protected %s selection to a new Session's first manual submission", async (mode) => {
    mocks.session = undefined;
    mocks.draft = "5m Original task";
    renderSessionPage();
    const props = mocks.composerProps as { onSend: (attachments: [], mode?: "goal" | "loop") => Promise<void> };
    await act(async () => { await props.onSend([], mode); });
    expect(mocks.sendTurn).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ nodeId: "embedded", skillSelection: { name: mode, path: `/actual-worker/${mode}/SKILL.md` } }));
  });

  it("waits for initial admission before querying the newly created session", async () => {
    mocks.session = undefined;
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "hello" } }]}>
        <Routes><Route element={<ChatSessionPage />} path="/sessions/:sessionId" /></Routes>
      </MemoryRouter>,
    );
    await waitFor(() => expect(mocks.sendText).toHaveBeenCalled());
    expect(mocks.sessionQueryEnabled).toBe(false);
    await act(async () => { resolveSend(); });
    await waitFor(() => expect(mocks.sessionQueryEnabled).toBe(true));
  });

  it.each(["success", "failure"])("clears pending admission after navigating away and returning (%s)", async (outcome) => {
    render(
      <MemoryRouter initialEntries={[{ pathname: "/sessions/sess-1", state: { initialPrompt: "hello" } }]}>
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
vi.mock("@/features/computers", () => ({ useComputers: () => ({ selectedId: apiMocks.node, select: vi.fn() }), useProductScope: () => "test-scope", useComputer: () => ({ data: { available: true } }) }));
