// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { FileInfo } from "@talos/sdk-browser";

import { FileDetailsPanel } from "./FileDetailsPanel";
import { useAppStore } from "@/shared/store/app-store";

const file: FileInfo = { fileId: "notes", originalFilename: "Guide.markdown", mimeType: "text/plain", sizeBytes: 20, scope: { type: "personal" }, tags: [], metadata: {}, createdAt: 1, updatedAt: 1 };
function setup(value = file) {
  render(<FileDetailsPanel file={value} isDeleting={false} isUpdating={false} onClose={vi.fn()} onDelete={vi.fn()} onUpdate={vi.fn()} />);
}
afterEach(() => {
  cleanup();
  useAppStore.setState({ workspaceWindows: [], workspaceActiveKey: null, workspaceOpen: false, workspaceExpanded: false });
});
it("opens the actual Markdown file in the shared right workspace without replacing its detail actions", () => {
  setup();
  fireEvent.click(screen.getByRole("button", { name: "Preview Markdown" }));
  expect(useAppStore.getState().workspaceWindows).toEqual([{ kind: "markdown", fileId: "notes" }]);
  expect(useAppStore.getState().workspaceOpen).toBe(true);
  expect(screen.getByRole("button", { name: "Save changes" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
});
it("does not offer Markdown preview for a PDF", () => {
  setup({ ...file, originalFilename: "Guide.pdf", mimeType: "application/pdf" });
  expect(screen.queryByRole("button", { name: "Preview Markdown" })).not.toBeInTheDocument();
});
