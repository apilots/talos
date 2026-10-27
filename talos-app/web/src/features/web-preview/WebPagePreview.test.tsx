// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { WebPagePreview } from "./WebPagePreview";
import { OpenWebpageButton } from "./OpenWebpageButton";
import { useAppStore } from "@/shared/store/app-store";

const external = vi.hoisted(() => vi.fn());
vi.mock("@/shared/lib/open-external", () => ({ openExternalUrl: external }));
beforeEach(() => {
  external.mockReset().mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup(); vi.restoreAllMocks();
  useAppStore.setState({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
});

it("creates a real blank webpage tab immediately without a modal or hidden input stealing focus", () => {
  const opener = document.createElement("button");
  document.body.append(opener); opener.focus();
  render(<OpenWebpageButton icon="globe" />);
  expect(opener).toHaveFocus();
  fireEvent.click(screen.getByRole("button", { name: "New webpage tab" }));
  const windows = useAppStore.getState().workspaceWindows;
  expect(windows).toHaveLength(1);
  expect(windows[0]).toMatchObject({ kind: "webpage", url: "" });
  expect(useAppStore.getState().workspaceOpen).toBe(true);
  expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  opener.remove();
});

it("allows multiple independent blank tabs through the plus entry", () => {
  render(<OpenWebpageButton />);
  fireEvent.click(screen.getByRole("button", { name: "New webpage tab" }));
  fireEvent.click(screen.getByRole("button", { name: "New webpage tab" }));
  const windows = useAppStore.getState().workspaceWindows;
  expect(windows).toHaveLength(2);
  expect(windows[0]).not.toEqual(windows[1]);
  expect(windows.every(window => window.kind === "webpage" && window.url === "")).toBe(true);
});

it("shows a usable new-tab page and visits a local service from its address bar", () => {
  const navigate = vi.fn();
  const view = render(<WebPagePreview url="" onNavigate={navigate} />);
  expect(screen.getByRole("heading", { name: "New tab" })).toBeInTheDocument();
  expect(view.container.querySelector("iframe")).toBeNull();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Refresh webpage" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "Enter an address" }));
  expect(screen.getByRole("textbox", { name: "Web address" })).toHaveFocus();
  fireEvent.change(screen.getByRole("textbox", { name: "Web address" }), { target: { value: "http://127.0.0.1:3000/preview" } });
  fireEvent.click(screen.getByRole("button", { name: "Go to address" }));
  expect(navigate).toHaveBeenCalledWith("http://127.0.0.1:3000/preview");
  view.rerender(<WebPagePreview url="http://127.0.0.1:3000/preview" onNavigate={navigate} />);
  expect(view.container.querySelector("iframe")).toHaveAttribute("src", "http://127.0.0.1:3000/preview");
  expect(screen.queryByRole("heading", { name: "New tab" })).not.toBeInTheDocument();
});

it("contains preview scripts, preserves the address, refreshes, and opens externally", async () => {
  const user = userEvent.setup();
  const { container } = render(<WebPagePreview url="https://example.com/" onNavigate={vi.fn()} />);
  const frame = container.querySelector("iframe")!;
  expect(frame).toHaveAttribute("src", "https://example.com/");
  expect(frame).toHaveAttribute("sandbox", "allow-scripts allow-forms");
  expect(frame).toHaveAttribute("referrerpolicy", "no-referrer");
  fireEvent.load(frame);
  expect(screen.getByText(/Some sites limit embedded previews/)).toBeInTheDocument();
  expect(screen.queryByText(/connected|loaded successfully/i)).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh webpage" }));
  expect(container.querySelector("iframe")).not.toBe(frame);
  await user.click(screen.getByRole("button", { name: "Open in options" }));
  await user.click(screen.getByRole("menuitem", { name: "Open in external browser" }));
  expect(external).toHaveBeenCalledWith("https://example.com/");
});

it("reloads the current destination when its address is submitted again", () => {
  const navigate = vi.fn();
  const { container } = render(<WebPagePreview url="https://example.com/" onNavigate={navigate} />);
  const frame = container.querySelector("iframe");
  fireEvent.click(screen.getByRole("button", { name: "Go to address" }));
  expect(navigate).toHaveBeenCalledWith("https://example.com/");
  expect(container.querySelector("iframe")).not.toBe(frame);
});

it("navigates via the workspace owner and rejects unsupported destinations", () => {
  const navigate = vi.fn();
  const { container } = render(<WebPagePreview url="https://example.com/" onNavigate={navigate} />);
  for (const value of ["data:text/html,unsafe", "javascript:alert(1)", "file:///tmp/index.html", "https://user:password@example.com/"]) {
    fireEvent.change(screen.getByRole("textbox", { name: "Web address" }), { target: { value } });
    fireEvent.click(screen.getByRole("button", { name: "Go to address" }));
    expect(screen.getByRole("alert")).toHaveTextContent("full http:// or https://");
    expect(navigate).not.toHaveBeenCalled();
    expect(container.querySelector("iframe")).toHaveAttribute("src", "https://example.com/");
  }
  fireEvent.change(screen.getByRole("textbox", { name: "Web address" }), { target: { value: " http://localhost:3000 " } });
  fireEvent.click(screen.getByRole("button", { name: "Go to address" }));
  expect(navigate).toHaveBeenCalledWith("http://localhost:3000/");
});

it("shows an actual external-browser error while keeping the address and refresh available", async () => {
  const user = userEvent.setup();
  external.mockRejectedValue(new Error("Bridge rejected request"));
  const { container } = render(<WebPagePreview url="https://example.com/" onNavigate={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Open in options" }));
  await user.click(screen.getByRole("menuitem", { name: "Open in external browser" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Unable to complete this action");
  expect(external).toHaveBeenCalledWith("https://example.com/");
  fireEvent.click(screen.getByRole("button", { name: "Refresh webpage" }));
  expect(container.querySelector("iframe")).toBeInTheDocument();
});
