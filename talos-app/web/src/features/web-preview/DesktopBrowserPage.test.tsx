// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { DesktopBrowserPage } from "./DesktopBrowserPage";
import { desktopBrowserRequest } from "./desktop-browser";
vi.mock("./desktop-browser", () => ({ desktopBrowserRequest: vi.fn(async() => null) }));
const tab = { id: "1", title: "Page", url: "https://example.test/", back: true, forward: false, loading: false };
beforeEach(() => {
  vi.clearAllMocks();useAppStore.setState({ desktopBrowser: { sessionId: "bound", revision: 1, error: null, tabs: [], revealTabId: null } });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 500, y: 100, width: 480, height: 600 } as DOMRect);
  vi.spyOn(HTMLElement.prototype, "getClientRects").mockReturnValue([{}] as unknown as DOMRectList);
});
afterEach(() => { cleanup();vi.unstubAllGlobals();vi.restoreAllMocks(); });
it.each([
  [" localhost:3000/preview?next=https://other.test/#section ", "http://localhost:3000/preview?next=https://other.test/#section"],
  ["http://example.test/page", "http://example.test/page"],
  ["https://example.test/page", "https://example.test/page"],
])("submits %s through Enter and the address button as %s", async (input, expected) => {
  const user = userEvent.setup();
  render(<DesktopBrowserPage tab={tab} selected />);
  const address = screen.getByRole("textbox", { name: "Web address" });
  fireEvent.change(address, { target: { value: input } });
  const request = { kind: "navigate", sessionId: "bound", tabId: "1", url: expected };
  await user.type(address, "{Enter}");
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith(request);
  vi.mocked(desktopBrowserRequest).mockClear();
  await user.click(screen.getByRole("button", { name: "Go to address" }));
  expect(desktopBrowserRequest).toHaveBeenCalledExactlyOnceWith(request);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("withdraws native presentation for collapse, overlays and minimization while retaining the target", async() => {
  const { rerender } = render(<dialog open><DesktopBrowserPage tab={tab} selected /></dialog>);
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "present", sessionId: "bound", tabId: "1", bounds: { x: 500, y: 100, width: 480, height: 600 } });
  const overlay = document.createElement("div");overlay.setAttribute("role", "dialog");document.body.append(overlay);
  await waitFor(() => expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "present", sessionId: "bound", tabId: null, bounds: null }));
  overlay.remove();await waitFor(() => expect(desktopBrowserRequest).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: "1" })));
  const visibility = vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");fireEvent(document, new Event("visibilitychange"));
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: null, bounds: null }));
  visibility.mockReturnValue("visible");fireEvent(document, new Event("visibilitychange"));
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: "1" }));
  vi.mocked(desktopBrowserRequest).mockClear();
  rerender(<dialog open><DesktopBrowserPage tab={tab} selected={false} /></dialog>);
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "present", sessionId: "bound", tabId: null, bounds: null });
});
it("preserves an edited address during host navigation and submits real native navigation controls", async() => {
  const { rerender } = render(<DesktopBrowserPage tab={tab} selected />);
  const address = screen.getByRole("textbox", { name: "Web address" });
  fireEvent.focus(address);fireEvent.change(address, { target: { value: "example.test/draft" } });
  rerender(<DesktopBrowserPage tab={{ ...tab, url: "https://example.test/elsewhere" }} selected />);
  expect(address).toHaveValue("example.test/draft");
  fireEvent.submit(address.closest("form")!);
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "navigate", sessionId: "bound", tabId: "1", url: "http://example.test/draft" });
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "back", sessionId: "bound", tabId: "1" });
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
  await act(async() => {});
});
