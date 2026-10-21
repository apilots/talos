// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TalosApiError, type FileInfo } from "@talos/sdk-browser";

import { MarkdownFilePreview } from "./MarkdownFilePreview";

const api = vi.hoisted(() => ({ read: vi.fn(), content: vi.fn(), openExternal: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ files: api }) }));
vi.mock("@/shared/lib/open-external", () => ({ openExternalUrl: api.openExternal }));

const file: FileInfo = {
  fileId: "notes", scope: { type: "personal" }, originalFilename: "Review.md",
  mimeType: "text/markdown", sizeBytes: 20, tags: [], metadata: {}, createdAt: 1, updatedAt: 1,
};
const clients: QueryClient[] = [];
function setup(scope = "alice", fileId = file.fileId) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  clients.push(client);
  const view = render(<QueryClientProvider client={client}><MarkdownFilePreview connectionKey={scope} fileId={fileId} /></QueryClientProvider>);
  return { ...view, client };
}
beforeEach(() => {
  vi.resetAllMocks();
  api.read.mockResolvedValue({ file });
  api.content.mockResolvedValue({ file, data: new TextEncoder().encode("# Product\n\n| Feature | Evidence |\n| --- | --- |\n| Browser | Real task |\n\n<script>unsafe()</script>") });
  api.openExternal.mockResolvedValue(undefined);
});
afterEach(() => { cleanup(); clients.splice(0).forEach(client => client.clear()); });

it("renders an authenticated file through the shared Markdown renderer without executing raw HTML", async () => {
  const { container } = setup();
  expect(await screen.findByRole("heading", { name: "Product" })).toBeInTheDocument();
  expect(screen.getByRole("table")).toHaveTextContent("Real task");
  expect(container.querySelector(".markdown-body")).not.toBeNull();
  expect(container.querySelector("script")).toBeNull();
  expect(api.read).toHaveBeenCalledWith({ fileId: "notes" });
  expect(api.content).toHaveBeenCalledWith({ fileId: "notes" });
});

it("shows a permission failure without downloading content and retries at the owning request", async () => {
  api.read.mockRejectedValueOnce(new TalosApiError({ code: "permissionDenied", message: "This file is not accessible.", localization: null }));
  setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("This file is not accessible.");
  expect(api.content).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { name: "Product" })).toBeInTheDocument();
});

it("keeps another account’s metadata and body out of the new scope", async () => {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  clients.push(client);
  const view = render(<QueryClientProvider client={client}><MarkdownFilePreview connectionKey="alice" fileId="notes" /></QueryClientProvider>);
  await screen.findByRole("heading", { name: "Product" });
  api.read.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "Access denied.", localization: null }));
  view.rerender(<QueryClientProvider client={client}><MarkdownFilePreview connectionKey="bob" fileId="notes" /></QueryClientProvider>);
  expect(screen.queryByRole("heading", { name: "Product" })).not.toBeInTheDocument();
  expect(await screen.findByRole("alert")).toHaveTextContent("Access denied.");
  expect(api.content).toHaveBeenCalledTimes(1);
});

it("reports invalid UTF-8 rather than displaying replacement characters", async () => {
  api.content.mockResolvedValue({ file, data: new Uint8Array([0xff, 0xfe]) });
  setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("not valid UTF-8");
});

it("shows an empty document and does not interpret another file type as Markdown", async () => {
  api.content.mockResolvedValue({ file, data: new Uint8Array() });
  const view = setup();
  expect(await screen.findByText("This document is empty.")).toBeInTheDocument();
  view.unmount();
  api.read.mockResolvedValue({ file: { ...file, fileId: "pdf", originalFilename: "Guide.pdf", mimeType: "application/pdf" } });
  setup("alice", "pdf");
  expect(await screen.findByText("This file does not support Markdown preview.")).toBeInTheDocument();
  expect(api.content).toHaveBeenCalledTimes(1);
});

it("opens only full web links externally and keeps relative links inside the document", async () => {
  api.content.mockResolvedValue({ file, data: new TextEncoder().encode("[Website](https://talos.love)\n\n[Relative](./other.md)") });
  setup();
  fireEvent.click(await screen.findByRole("link", { name: "Website" }));
  await waitFor(() => expect(api.openExternal).toHaveBeenCalledWith("https://talos.love/"));
  fireEvent.click(screen.getByRole("link", { name: "Relative" }));
  expect(screen.getByRole("alert")).toHaveTextContent("Only full HTTP(S)");
  expect(api.openExternal).toHaveBeenCalledTimes(1);
});

it("contains middle-click and keyboard link activation in the same external-opening boundary", async () => {
  api.content.mockResolvedValue({ file, data: new TextEncoder().encode("[Website](https://talos.love)") });
  setup();
  const link = await screen.findByRole("link", { name: "Website" });
  const middleClick = new MouseEvent("auxclick", { bubbles: true, cancelable: true, button: 1 });
  fireEvent(link, middleClick);
  expect(middleClick.defaultPrevented).toBe(true);
  expect(api.openExternal).toHaveBeenCalledWith("https://talos.love/");
  // Keyboard Enter activates an anchor with a click of detail 0.
  const keyboardClick = new MouseEvent("click", { bubbles: true, cancelable: true, detail: 0 });
  fireEvent(link, keyboardClick);
  expect(keyboardClick.defaultPrevented).toBe(true);
  expect(api.openExternal).toHaveBeenCalledTimes(2);
});
