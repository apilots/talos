// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { DesktopBrowserPage } from "./DesktopBrowserPage";
import { desktopBrowserRequest } from "./desktop-browser";
vi.mock("./desktop-browser", () => ({ desktopBrowserRequest: vi.fn(async() => null) }));
const tab = { id: "1", title: "Page", url: "https://example.test/", back: true, forward: false, loading: false };
beforeEach(() => {
  vi.clearAllMocks();useAppStore.setState({ desktopBrowser: { sessionId: "bound", revision: 1, error: null } });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ x: 500, y: 100, width: 480, height: 600 } as DOMRect);
  vi.spyOn(HTMLElement.prototype, "getClientRects").mockReturnValue([{}] as unknown as DOMRectList);
});
afterEach(() => { cleanup();vi.unstubAllGlobals();vi.restoreAllMocks(); });
it("withdraws native presentation for collapse, overlays and minimization while retaining the target", async() => {
  const { rerender } = render(<dialog open><DesktopBrowserPage tab={tab} selected /></dialog>);
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "present", sessionId: "bound", tabId: "1", bounds: { x: 500, y: 100, width: 480, height: 600 } });
  const overlay = document.createElement("div");overlay.setAttribute("role", "dialog");document.body.append(overlay);
  await waitFor(() => expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "present", sessionId: "bound", tabId: null, bounds: null }));
  overlay.remove();await waitFor(() => expect(desktopBrowserRequest).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: "1" })));
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");fireEvent(document, new Event("visibilitychange"));
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith(expect.objectContaining({ tabId: null, bounds: null }));
  rerender(<dialog open><DesktopBrowserPage tab={tab} selected={false} /></dialog>);
  expect(vi.mocked(desktopBrowserRequest).mock.calls.some(([request]) => request.kind === "close")).toBe(false);
});
it("preserves an edited address during host navigation and submits real native navigation controls", async() => {
  const { rerender } = render(<DesktopBrowserPage tab={tab} selected />);
  const address = screen.getByRole("textbox", { name: "Web address" });
  fireEvent.focus(address);fireEvent.change(address, { target: { value: "example.test/draft" } });
  rerender(<DesktopBrowserPage tab={{ ...tab, url: "https://example.test/elsewhere" }} selected />);
  expect(address).toHaveValue("example.test/draft");
  fireEvent.submit(address.closest("form")!);
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "navigate", sessionId: "bound", tabId: "1", url: "https://example.test/draft" });
  fireEvent.click(screen.getByRole("button", { name: "Back" }));
  expect(desktopBrowserRequest).toHaveBeenLastCalledWith({ kind: "back", sessionId: "bound", tabId: "1" });
  expect(screen.getByRole("button", { name: "Forward" })).toBeDisabled();
  await act(async() => {});
});
