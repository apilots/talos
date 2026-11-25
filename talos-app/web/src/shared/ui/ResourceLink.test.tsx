// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it } from "vitest";
import { MarkdownRenderer } from "@/widgets/message-list/MarkdownRenderer";
import { useAppStore } from "@/shared/store/app-store";
afterEach(cleanup);
beforeEach(() => useAppStore.setState({ workspaceOwner: null, workspaceSnapshots: {}, workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false }));
it("connects registered file anchors while preserving the existing image renderer", () => {
  render(<MarkdownRenderer text="[Report](talos-file://report)" className="markdown-body" />);
  fireEvent.click(screen.getByRole("link", { name: "Report" }));
  expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "file", fileId: "report" }]);
});
it("contains middle-click and keyboard activation in the same opening boundary", () => {
  render(<MarkdownRenderer text="[Website](https://example.test)" className="markdown-body" />);
  const link = screen.getByRole("link", { name: "Website" });
  const middle = new MouseEvent("auxclick", { button: 1, bubbles: true, cancelable: true });
  fireEvent(link, middle); expect(middle.defaultPrevented).toBe(true);
  fireEvent.click(link, { detail: 0 });
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
});
it("reports unsupported relative links without leaving the application", async () => {
  render(<MarkdownRenderer text="[Relative](./other.md)" className="markdown-body" />);
  fireEvent.click(screen.getByRole("link", { name: "Relative" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Unable to open this link");
});

it("preserves the browser action for safe mail links already supported by Markdown", () => {
  render(<MarkdownRenderer text="[Email](mailto:support@example.test)" className="markdown-body" />);
  const link = screen.getByRole("link", { name: "Email" });
  let intercepted: boolean | undefined;
  const observe = (event: Event) => { intercepted = event.defaultPrevented; event.preventDefault(); };
  // Observe after React dispatch; prevent jsdom from attempting native mail navigation.
  document.body.addEventListener("click", observe, { once: true });
  fireEvent.click(link);
  expect(link).toHaveAttribute("href", "mailto:support@example.test");
  expect(intercepted).toBe(false);
});

it("opens root-level file line citations from Markdown links in a session", () => {
  useAppStore.getState().setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "session-1" } }, true);
  render(<MarkdownRenderer text="[Report line](report.md:12)" className="markdown-body" />);
  const link = screen.getByRole("link", { name: "Report line" });
  expect(link).toHaveAttribute("href", "report.md:12");
  fireEvent.click(link);
  expect(useAppStore.getState().workspaceWindows).toEqual([
    { kind: "sessionFile", sessionId: "session-1", path: "report.md", position: { kind: "text", resourcePath: null, line: 12, encoding: "utf-8" } },
  ]);
});

it("opens clickable inline code file paths and URLs in session messages without affecting fenced blocks or code symbols", () => {
  useAppStore.getState().setWorkspaceContext({ scope: "actor", context: { kind: "session", sessionId: "session-1" } }, true);
  const text = [
    "Inspect `output/report.html`, `src/lib.rs:27`, and `https://example.test/docs`.",
    "Skip `console.log`, `Web/Desktop`, `text/html`, and `/status`.",
    "Keep [`linked.md`](linked.md) as a single anchor.",
    "",
    "```",
    "output/fenced.html",
    "```",
  ].join("\n");
  render(<MarkdownRenderer text={text} className="markdown-body" interactiveCodeReferences />);

  fireEvent.click(screen.getByRole("link", { name: "output/report.html" }));
  fireEvent.click(screen.getByRole("link", { name: "src/lib.rs:27" }));
  fireEvent.click(screen.getByRole("link", { name: "https://example.test/docs" }));
  expect(useAppStore.getState().workspaceWindows).toEqual([
    { kind: "sessionFile", sessionId: "session-1", path: "output/report.html" },
    { kind: "sessionFile", sessionId: "session-1", path: "src/lib.rs", position: { kind: "text", resourcePath: null, line: 27, encoding: "utf-8" } },
    expect.objectContaining({ kind: "webpage", url: "https://example.test/docs" }),
  ]);

  expect(screen.getAllByRole("link", { name: "linked.md" })).toHaveLength(1);
  expect(screen.queryByRole("link", { name: "output/fenced.html" })).toBeNull();
  for (const symbol of ["console.log", "Web/Desktop", "text/html", "/status"]) {
    expect(screen.queryByRole("link", { name: symbol })).toBeNull();
  }
});

it("intercepts protocol-relative links without navigating the host window", async () => {
  render(<MarkdownRenderer text="[Proto](//example.test/page)" className="markdown-body" />);
  const link = screen.getByRole("link", { name: "Proto" });
  let intercepted: boolean | undefined;
  const observe = (event: Event) => { intercepted = event.defaultPrevented; event.preventDefault(); };
  document.body.addEventListener("click", observe, { once: true });
  fireEvent.click(link);
  expect(intercepted).toBe(true);
  expect(await screen.findByRole("alert")).toHaveTextContent("Unable to open this link");
});

it("leaves inline code non-interactive by default and restricts session file paths outside a session", () => {
  const { container, rerender } = render(<MarkdownRenderer text="See `output/report.html` and `https://example.test`." className="markdown-body" />);
  expect(screen.queryByRole("link", { name: "output/report.html" })).toBeNull();
  expect(screen.queryByRole("link", { name: "https://example.test" })).toBeNull();

  rerender(<MarkdownRenderer text={"See `output/report.html`, `talos-file://doc-1`, and `https://example.test`.\n\n```\noutput/fenced.html\n```"} className="markdown-body" interactiveCodeReferences />);
  expect(screen.queryByRole("link", { name: "output/report.html" })).toBeNull();
  expect(screen.getByRole("link", { name: "talos-file://doc-1" })).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "https://example.test" })).toBeInTheDocument();
  expect(container.querySelector("pre")).not.toHaveAttribute("node");
  expect(container.querySelector("code")).not.toHaveAttribute("node");
});
