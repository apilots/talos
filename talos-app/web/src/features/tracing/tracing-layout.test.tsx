// @vitest-environment jsdom
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { installTracingDom, cleanupTracingDom } from "./tracing.test-utils";
import { i18n } from "@/shared/i18n/setup";
import { SessionTracingPanel } from "./components/SessionTracingPanel";
import { history, identity, request } from "./tracing.fixtures";

const mocks = vi.hoisted(() => ({
  capabilities: vi.fn(),
  readTurn: vi.fn(),
  readRequest: vi.fn(),
}));
vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ trajectory: mocks }),
}));
vi.mock("@/features/chat", () => ({
  useConnectionKey: () => "layout-connection",
}));
const resizeCallbacks = new Map<Element, (width: number) => void>();
function resize(root: HTMLElement, width: number) {
  act(() => resizeCallbacks.get(root)!(width));
}
function mount(savedHistory = history) {
  render(
    <QueryClientProvider
      client={
        new QueryClient({ defaultOptions: { queries: { retry: false } } })
      }
    >
      <SessionTracingPanel sessionId="s" visible history={savedHistory} />
    </QueryClientProvider>,
  );
  return screen.getByRole("region", { name: "Execution trace" });
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  resizeCallbacks.clear();
  installTracingDom({ narrow: false });
  vi.stubGlobal(
    "ResizeObserver",
    class {
      callback: ResizeObserverCallback;
      constructor(callback: ResizeObserverCallback) {
        this.callback = callback;
      }
      observe(target: Element) {
        resizeCallbacks.set(target, (width) => {
          const contentSize = {
            inlineSize: Math.max(0, width - 32),
            blockSize: 600,
          };
          this.callback(
            [
              {
                target,
                borderBoxSize: [{ inlineSize: width, blockSize: 600 }],
                contentBoxSize: [contentSize],
                devicePixelContentBoxSize: [contentSize],
                contentRect: DOMRectReadOnly.fromRect({
                  width: contentSize.inlineSize,
                  height: 600,
                }),
              },
            ],
            this,
          );
        });
      }
      unobserve() {}
      disconnect() {}
    },
  );
  mocks.capabilities.mockResolvedValue({
    supported: true,
  });
  mocks.readTurn.mockResolvedValue({
    identity,
    clockSegments: [],
    data: [{ type: "modelRequest", request }],
    nextCursor: null,
  });
  const missing = null;
  mocks.readRequest.mockResolvedValue({
    identity,
    request,
    prompt: missing,
    tools: missing,
    parameters: missing,
    outputs: [],
  });
});
afterEach(() => {
  cleanupTracingDom();
});

it("uses a 320px container inside a wide viewport and opens narrow details only after a click", async () => {
  const root = mount();
  resize(root, 320);
  const row = await screen.findByRole("button", { name: /LLM.*Call 1/ });
  expect(root.dataset.narrow).toBe("true");
  expect(
    screen.queryByRole("button", { name: "Close trace details" }),
  ).toBeNull();
  expect(document.activeElement).toBe(document.body);
  resize(root, 0);
  expect(root.dataset.narrow).toBe("true");
  row.focus();
  fireEvent.click(row);
  const close = await screen.findByRole("button", {
    name: "Close trace details",
  });
  await waitFor(() => expect(document.activeElement).toBe(close));
  expect(
    root
      .querySelector(".tracing-workbench")
      ?.classList.contains("mobile-detail"),
  ).toBe(true);
  fireEvent.keyDown(close, { key: "Escape" });
  expect(document.activeElement).toBe(row);
  expect(
    root
      .querySelector(".tracing-workbench")
      ?.classList.contains("mobile-detail"),
  ).toBe(false);
});

it("restores a connected trigger or the current Turn summary when hiding a wide Inspector", async () => {
  const other = structuredClone(history.data[0]);
  other.identity = { ...identity, turnId: "other" };
  const root = mount({ ...history, data: [history.data[0], other] });
  resize(root, 780);
  const row = await screen.findByRole("button", { name: /LLM.*Call 1/ });
  await screen.findByRole("tab", { name: "Summary" });
  expect(document.activeElement).toBe(document.body);
  row.focus();
  fireEvent.click(row);
  screen.getByRole("button", { name: "Close trace details" }).focus();
  resize(root, 320);
  expect(document.activeElement).toBe(row);
  expect(
    root
      .querySelector(".tracing-workbench")
      ?.classList.contains("mobile-detail"),
  ).toBe(false);
  row.blur();
  resize(root, 1000);
  expect(document.activeElement).toBe(document.body);

  const current = root.querySelector<HTMLDetailsElement>(".tracing-turn")!;
  current.open = true;
  fireEvent(current, new Event("toggle"));
  await waitFor(() => expect(row.isConnected).toBe(false));
  await screen.findByRole("tab", { name: "Summary" });
  screen.getByRole("button", { name: "Close trace details" }).focus();
  resize(root, 320);
  expect(document.activeElement).toBe(current.querySelector("summary"));
});
