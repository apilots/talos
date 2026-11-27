// @vitest-environment jsdom
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Link, MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import "@/shared/i18n/setup";
import "@testing-library/jest-dom/vitest";
import { useAppStore } from "@/shared/store/app-store";
import { createDefaultProjects } from "@/shared/types/domain";
import { createBrowserTalosClient } from "@talos/sdk-browser";
import { setTalosClientFactory, type TalosWebClient } from "@/shared/api/client";
import { render, composer, startTurn, computer, setActiveProjectIdMock } from "./ChatHomePage.test-support";
import { ChatHomePage } from "./ChatHomePage";

function LocationStateProbe() {
  const location = useLocation();
  return <pre data-testid="location-state">{JSON.stringify(location.state)}</pre>;
}

describe("ChatHomePage", () => {
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

  it.each([false, true])("admits the first Turn before navigating, including HTTP UUID capabilities (http=%s)", async (http) => {
    if (http) vi.stubGlobal("crypto", { getRandomValues: crypto.getRandomValues.bind(crypto) });
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
    await act(() => { resolve({ data: [{ name: "goal", scope: "system", enabled: true, read_only: true, path: "/real/goal/SKILL.md" }] });  return Promise.resolve(); });
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
    await act(() => { accept({ turnId: "turn-1", messageId: "message-1" });  return Promise.resolve(); });
    await waitFor(() => expect(screen.getByTestId("location-state")).toHaveTextContent("null"));
    expect(startTurn).toHaveBeenCalledTimes(2);
  });

  it.each(["unchanged", "text", "permission"])("restores SDK retry identity after a lost ACK and Home remount (edited=%s)", async (edited) => {
    composer.real = true;
    const accepted = new Map<string, string>();
    const keys: string[] = [];
    const request = vi.fn<typeof fetch>( (_url, init) => {
      if (!(_url instanceof Request ? _url.url : _url.toString()).endsWith("/v1/turns/start")) return Promise.resolve(Response.json({ status: "success", data: { data: [] } }));
      const key = new Headers(init?.headers).get("idempotency-key")!;
      const body = (typeof init?.body === "string" ? init.body : (() => { throw new Error("Expected a JSON request body."); })());
      keys.push(key);
      if (accepted.has(key)) expect(body).toBe(accepted.get(key));
      accepted.set(key, body);
      if (keys.length === 1) return Promise.reject(new TypeError("Server response lost"));
      return Promise.resolve(Response.json({ status: "success", data: { turnId: "turn-1", messageId: "message-1" } }));
    });
    setTalosClientFactory(() => createBrowserTalosClient({ baseUrl: "https://server.example", fetch: request }));
    render(<MemoryRouter initialEntries={[{ pathname: "/", state: { initialDraft: "Original task" } }]}><Routes>
      <Route path="/" element={<><ChatHomePage /><Link to="/settings">Leave Home</Link></>} />
      <Route path="/settings" element={<Link to="/">Return Home</Link>} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>);
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("alert");
    expect(screen.queryByTestId("location-state")).toBeNull();
    fireEvent.click(screen.getByRole("link", { name: "Leave Home" }));
    fireEvent.click(screen.getByRole("link", { name: "Return Home" }));
    expect(screen.getByRole("textbox")).toHaveValue("Original task");
    if (edited === "text") fireEvent.change(screen.getByRole("textbox"), { target: { value: "Edited task" } });
    if (edited === "permission") {
      fireEvent.click(screen.getByRole("button", { name: "Full access" }));
      fireEvent.click(screen.getByRole("button", { name: /^Ask for approval/ }));
    }
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByTestId("location-state");
    expect(keys).toHaveLength(2);
    expect(accepted.size).toBe(edited === "unchanged" ? 1 : 2);
  });

  it.each([["remount", "goal"], ["remount", "loop"], ["reload", "goal"], ["reload", "loop"]] as const)("restores the complete unacknowledged submission after %s (%s)", async (recovery, mode) => {
    composer.real = true;
    const title = mode === "goal" ? "Goal" : "Loop";
    const file = { fileId: "server-file", originalFilename: "report.png", mimeType: "image/png", sizeBytes: 8,
      scope: { type: "personal" as const }, tags: [], metadata: {}, createdAt: 1, updatedAt: 1 };
    const sent: Array<{ key: string | null; body: string }> = [];
    const request = vi.fn<typeof fetch>( (_url, init) => {
      sent.push({ key: new Headers(init?.headers).get("idempotency-key"), body: (typeof init?.body === "string" ? init.body : (() => { throw new Error("Expected a JSON request body."); })()) });
      if (sent.length === 1) return Promise.reject(new TypeError("Server response lost after admission"));
      return Promise.resolve(Response.json({ status: "success", data: { turnId: "turn-1", messageId: "message-1" } }));
    });
    setTalosClientFactory(() => ({ ...createBrowserTalosClient({ baseUrl: "https://server.example", fetch: request }),
      files: { upload: vi.fn().mockResolvedValue({ file }), read: vi.fn().mockResolvedValue({ file }) },
      skills: { list: vi.fn().mockResolvedValue({ data: [{ name: mode, scope: "system", path: `/worker/${mode}/SKILL.md`, enabled: true, read_only: true }] }) },
      automation: { commandCatalog: vi.fn().mockResolvedValue({ data: [{ mode, command: `/${mode}`, description: title }] }) },
    }) as unknown as TalosWebClient);
    useAppStore.getState().setActiveProjectId("project-1");
    const home = <MemoryRouter><Routes>
      <Route path="/" element={<><ChatHomePage /><Link to="/settings">Settings</Link></>} />
      <Route path="/settings" element={<Link to="/">Return Home</Link>} />
      <Route path="/sessions/:sessionId" element={<LocationStateProbe />} />
    </Routes></MemoryRouter>;
    const view = render(home);
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "Check the report" } });
    fireEvent.click(screen.getByRole("button", { name: /^Tencent HY3 Free ·/ }));
    fireEvent.click(screen.getByRole("button", { name: /Select model: Tencent HY3/ }));
    fireEvent.click(screen.getByRole("button", { name: /Change Provider/ }));
    fireEvent.click(screen.getByRole("button", { name: /^Ollama$/i }));
    fireEvent.click(screen.getByRole("button", { name: "Llama 3.3" }));
    fireEvent.keyDown(screen.getByRole("slider", { name: "Reasoning depth" }), { key: "Home" });
    fireEvent.keyDown(document, { key: "Escape" });
    fireEvent.change(view.container.querySelector('input[type="file"]')!, { target: { files: [new File(["image"], "report.png", { type: "image/png" })] } });
    await screen.findByRole("button", { name: "Remove report.png" });
    fireEvent.click(screen.getByRole("button", { name: "Full access" }));
    fireEvent.click(screen.getByRole("button", { name: /^Ask for approval/ }));
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    fireEvent.click(await screen.findByRole("button", { name: title }));
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByRole("alert");
    if (recovery === "remount") {
      fireEvent.click(screen.getByRole("link", { name: "Settings" }));
      fireEvent.click(screen.getByRole("link", { name: "Return Home" }));
    } else {
      view.unmount();
      const storage = useAppStore.persist.getOptions().storage!;
      const persisted = await storage.getItem("talos-web-ui");
      useAppStore.setState(useAppStore.getInitialState());
      await storage.setItem("talos-web-ui", persisted!);
      await useAppStore.persist.rehydrate();
      render(home);
    }
    expect(screen.getByRole("textbox")).toHaveValue("Check the report");
    expect(screen.getByRole("button", { name: "Ask for approval" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Llama 3.3 · Off" })).toBeVisible();
    expect(screen.getByRole("button", { name: /alpha/ })).toBeVisible();
    expect(screen.getByTestId("automation-mode-chip")).toHaveTextContent(title);
    await screen.findByRole("button", { name: "Remove report.png" });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByTestId("location-state");
    expect(sent).toHaveLength(2);
    expect(sent[1]).toEqual(sent[0]);
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
    await act(() => { accept({ turnId: "turn-1", messageId: "message-1" });  return Promise.resolve(); });
    expect(screen.getByText("Settings destination")).toBeVisible();
    expect(useAppStore.getState().composerDrafts[key]).toBeUndefined();
  });

});
