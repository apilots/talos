// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import type { AppProtocol as API } from "@talos/sdk-core";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { installTracingDom, cleanupTracingDom } from "./tracing.test-utils";
import { i18n } from "@/shared/i18n/setup";
import { SessionTracingPanel } from "./components/SessionTracingPanel";
import { history, identity, request, ref, prompt } from "./tracing.fixtures";
const mocks = vi.hoisted(() => ({
  capabilities: vi.fn(),
  readTurn: vi.fn(),
  readRequest: vi.fn(),
  readTool: vi.fn(),
  payload: vi.fn(),
}));
vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ trajectory: mocks }),
}));
vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "test-connection",
}));
function mount(visible = true) {
  const cache = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const panel = (isVisible: boolean) => (
    <QueryClientProvider client={cache}>
      <SessionTracingPanel
        sessionId="s"
        visible={isVisible}
        history={history}
      />
    </QueryClientProvider>
  );
  const result = render(panel(visible));
  return (next: boolean) => result.rerender(panel(next));
}
beforeEach(async () => {
  vi.clearAllMocks();
  installTracingDom({ narrow: true });
  await i18n.changeLanguage("en");
  mocks.capabilities.mockResolvedValue({
    supported: true,
  });
  mocks.readTurn.mockResolvedValue({
    identity,
    clockSegments: [],
    data: [{ type: "modelRequest", request }],
    nextCursor: null,
  });
  mocks.readRequest.mockResolvedValue({
    identity,
    request,
    prompt: ref,
    tools: null,
    parameters: null,
    outputs: [],
  });
  mocks.payload.mockResolvedValue(prompt);
});
afterEach(() => {
  cleanupTracingDom();
});
describe("Session tracing", () => {
  it("queries only while visible and cancels metadata paging and pending bodies", async () => {
    let finishMetadata!: (page: API.TrajectoryReadTurnResponse) => void;
    mocks.readTurn.mockImplementationOnce(() => new Promise((resolve) => { finishMetadata = resolve; }));
    mocks.payload.mockImplementationOnce(() => new Promise(() => {}));
    const setVisible = mount(false);
    expect(mocks.capabilities).not.toHaveBeenCalled();
    expect(mocks.readTurn).not.toHaveBeenCalled();
    setVisible(true);
    await waitFor(() => expect(mocks.readTurn).toHaveBeenCalledTimes(1));
    const metadataSignal = mocks.readTurn.mock.calls[0][1] as AbortSignal;
    setVisible(false);
    await waitFor(() => expect(metadataSignal.aborted).toBe(true));
    await act(async () => finishMetadata({ identity, clockSegments: [], data: [], nextCursor: "next" }));
    expect(mocks.readTurn).toHaveBeenCalledTimes(1);
    setVisible(true);
    fireEvent.click(
      await screen.findByRole("button", { name: /LLM.*Call 1/ }),
    );
    fireEvent.click(screen.getByRole("tab", { name: "Prompt" }));
    await waitFor(() => expect(mocks.payload).toHaveBeenCalledTimes(1));
    const signal = mocks.payload.mock.calls[0][2] as AbortSignal;
    expect(signal.aborted).toBe(false);
    setVisible(false);
    await waitFor(() => expect(signal.aborted).toBe(true));
  });
  it("keeps legacy feedback and original metrics while missing observations stay unknown", async () => {
    mocks.readTurn.mockResolvedValue({
      identity,
      clockSegments: [],
      data: null,
      nextCursor: null,
    });
    mount();
    expect(
      (await screen.findAllByText(/Request details were not recorded/)).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("阶段反馈保持原文")).toBeTruthy();
    expect(screen.getByText("Turn 1 · original totals")).toBeTruthy();
    expect(mocks.readRequest).not.toHaveBeenCalled();
    expect(mocks.payload).not.toHaveBeenCalled();
  });
  it("loads selected bodies only and restores focus after explicit narrow close", async () => {
    mocks.readRequest.mockRejectedValueOnce(new Error("temporary request read failure"));
    mocks.payload.mockRejectedValueOnce(new Error("temporary body read failure"));
    mount();
    const row = await screen.findByRole("button", { name: /LLM.*Call 1/ });
    expect(mocks.readRequest).not.toHaveBeenCalled();
    row.focus();
    fireEvent.click(row);
    await waitFor(() =>
      expect(document.activeElement).toBe(
        screen.getByRole("button", { name: "Close trace details" }),
      ),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Retry read" }));
    await within(screen.getByRole("tabpanel")).findByText("1 s");
    fireEvent.click(screen.getByRole("tab", { name: "Prompt" }));
    expect((await screen.findAllByText("Read failed")).length).toBeGreaterThan(
      0,
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry read" }));
    await within(screen.getByRole("tabpanel")).findByText("exact prompt");
    expect(mocks.payload).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("Read failed")).toBeNull();
    expect(screen.queryByRole("combobox", { name: "Select call record" })).toBeNull();
    expect(screen.getByRole("tab", { name: "Prompt" })).toBeTruthy();
    fireEvent.keyDown(
      screen.getByRole("button", { name: "Close trace details" }),
      { key: "Escape" },
    );
    expect(document.activeElement).toBe(row);
    expect(screen.queryByRole("tab", { name: "Prompt" })).toBeNull();
  });
  it("initial render does not move keyboard focus and preserves feedback when switching locale", async () => {
    mount();
    await screen.findByText(/Model request 1/);
    expect(document.activeElement).toBe(document.body);
    await i18n.changeLanguage("zh-CN");
    expect(await screen.findByText("执行轨迹")).toBeTruthy();
    expect(screen.getByText("阶段反馈保持原文")).toBeTruthy();
  });
  it("supports wide keyboard inspection without moving initial focus or preloading Prompt", async () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));
    mount();
    await screen.findByRole("tab", { name: "Summary" });
    await within(screen.getByRole("tabpanel")).findByText("1 s");
    expect(document.activeElement).toBe(document.body);
    expect(mocks.payload).not.toHaveBeenCalled();
    const summary = screen.getByRole("tab", { name: "Summary" });
    fireEvent.keyDown(summary, { key: "End" });
    expect(document.activeElement).toBe(
      screen.getByRole("tab", { name: "Timing" }),
    );
    expect(document.activeElement?.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(document.activeElement!, { key: "Home" });
    expect(document.activeElement).toBe(summary);
    fireEvent.keyDown(summary, { key: "ArrowLeft" });
    expect(document.activeElement).toBe(
      screen.getByRole("tab", { name: "Timing" }),
    );
    expect(Element.prototype.scrollIntoView).toHaveBeenCalledWith({
      block: "nearest",
      inline: "nearest",
    });
    fireEvent.click(screen.getByRole("tab", { name: "Prompt" }));
    await within(screen.getByRole("tabpanel")).findByText("exact prompt");
    const input = screen.getByRole("button", { name: /SYSTEM.*exact prompt/ });
    fireEvent.click(input);
    expect(input.getAttribute("aria-pressed")).toBe("true");
    const response = screen.getByRole("button", {
      name: /ASSISTANT.*Response/,
    });
    fireEvent.click(response);
    expect(response.getAttribute("aria-pressed")).toBe("true");
    expect(input.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(screen.getByRole("tab", { name: "Prompt" }));
    fireEvent.click(
      screen.getByRole("button", { name: "Close trace details" }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("tab", { name: "Prompt" })).toBeNull(),
    );
  });
  it("opens Timing from an actual LLM interval", async () => {
    mocks.readTurn.mockResolvedValue({
      identity,
      clockSegments: [
        {
          segmentId: "seg",
          sourceNodeId: "worker",
          startedAt: 1,
          relation: "turnStart",
          observedThroughOffsetMs: 1500,
        },
      ],
      data: [{ type: "modelRequest", request: {
        ...request, callRecords: [...request.callRecords, {
          ...request.callRecords[0], callRecordId: "unfinished", ordinal: 2, observedState: "incomplete",
          timing: { ...request.callRecords[0].timing!, interval: { ...request.callRecords[0].timing!.interval, endedOffsetMs: null, elapsedMs: null } },
        }],
      } }],
      nextCursor: null,
    });
    mount();
    expect(await screen.findByRole("button", { name: "Turn 1 · LLM · Model request 1 · Call 2 · End time was not recorded" })).toBeTruthy();
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Turn 1 · LLM · Model request 1 · Call 1",
      }),
    );
    expect(
      screen.getByRole("tab", { name: "Timing" }).getAttribute("aria-selected"),
    ).toBe("true");
    await within(screen.getByRole("tabpanel")).findByText("seg");
  });
});
