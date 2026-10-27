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
