// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TalosApiError, type FileInfo } from "@talos/sdk-browser";

import { FilePreview } from "./FilePreview";

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
  const view = render(<QueryClientProvider client={client}><FilePreview instanceKey="test-context" connectionKey={scope} fileId={fileId} /></QueryClientProvider>);
  return { ...view, client };
}
beforeEach(() => {
  vi.resetAllMocks();
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: vi.fn(() => "blob:test-preview") });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: vi.fn() });
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
  const view = render(<QueryClientProvider client={client}><FilePreview instanceKey="test-context" connectionKey="alice" fileId="notes" /></QueryClientProvider>);
  await screen.findByRole("heading", { name: "Product" });
  api.read.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "Access denied.", localization: null }));
  view.rerender(<QueryClientProvider client={client}><FilePreview instanceKey="test-context" connectionKey="bob" fileId="notes" /></QueryClientProvider>);
  expect(screen.queryByRole("heading", { name: "Product" })).not.toBeInTheDocument();
  expect(await screen.findByRole("alert")).toHaveTextContent("Access denied.");
  expect(api.content).toHaveBeenCalledTimes(1);
});

it("reports invalid UTF-8 rather than displaying replacement characters", async () => {
  api.content.mockResolvedValue({ file, data: new Uint8Array([0xff, 0xfe]) });
  setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("not valid UTF-8");
});
it("stops refresh at a metadata permission failure instead of using stale metadata to read content", async () => {
  setup();
  await screen.findByRole("heading", { name: "Product" });
  api.read.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "Access revoked.", localization: null }));
  fireEvent.click(screen.getByRole("button", { name: "Refresh webpage" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Access revoked.");
  expect(api.content).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Download file" })).toBeDisabled();
});

it("shows an empty document and a native PDF frame instead of Markdown", async () => {
  api.content.mockResolvedValue({ file, data: new Uint8Array() });
  const view = setup();
  expect(await screen.findByText("This document is empty.")).toBeInTheDocument();
  view.unmount();
  const pdf = { ...file, fileId: "pdf", originalFilename: "Guide.pdf", mimeType: "application/pdf" };
  api.read.mockResolvedValue({ file: pdf });
  api.content.mockResolvedValue({ file: pdf, data: new Uint8Array([1, 2]) });
  const native = setup("alice", "pdf");
  expect(await screen.findByTitle("Guide.pdf")).toHaveAttribute("src", "blob:test-preview");
  expect(screen.getByRole("textbox", { name: "File location" })).toHaveAttribute("readonly");
  expect(URL.createObjectURL).toHaveBeenCalledWith(expect.objectContaining({ type: "application/pdf" }));
  native.unmount();
  expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:test-preview");
});

it("uses the same sandbox for HTML and images and rereads on refresh", async () => {
  const html = { ...file, originalFilename: "report.html", mimeType: "text/html" };
  api.read.mockResolvedValue({ file: html });
  api.content.mockResolvedValue({ file: html, data: new TextEncoder().encode("<h1>Report</h1>") });
  setup();
  expect(await screen.findByTitle("report.html")).toHaveAttribute("sandbox", "allow-scripts allow-forms");
  expect(screen.getByText(/Self-contained HTML/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Refresh webpage" }));
  await waitFor(() => expect(api.content).toHaveBeenCalledTimes(2));
});

it("uses server MIME rather than an HTML filename and offers authenticated download", async () => {
  api.read.mockResolvedValue({ file: { ...file, originalFilename: "report.html", mimeType: "application/zip" } });
  setup();
  expect(await screen.findByText(/This format cannot be displayed/)).toBeInTheDocument();
  expect(api.content).not.toHaveBeenCalled();
  api.content.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "Download denied.", localization: null }));
  fireEvent.click(screen.getByRole("button", { name: "Download file" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Download denied.");
});

it("renders stored text as escaped content rather than executing markup", async () => {
  const code = { ...file, originalFilename: "data.csv", mimeType: "text/csv" };
  api.read.mockResolvedValue({ file: code });
  api.content.mockResolvedValue({ file: code, data: new TextEncoder().encode("<script>unsafe()</script>") });
  const view = setup();
  expect(await screen.findByText("<script>unsafe()</script>")).toBeInTheDocument();
  expect(view.container.querySelector("script")).toBeNull();
});

it("refreshes only the owning context when the same HTML file is open in two contexts", async () => {
  const html = { ...file, originalFilename: "report.html", mimeType: "text/html" };
  api.read.mockResolvedValue({ file: html });
  api.content.mockImplementation(async () => ({ file: html, data: new TextEncoder().encode("<input>") }));
  let urls = 0;
  vi.mocked(URL.createObjectURL).mockImplementation(() => `blob:context-${++urls}`);
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } }); clients.push(client);
  render(<QueryClientProvider client={client}><div data-testid="one"><FilePreview connectionKey="actor" fileId="notes" instanceKey="session-one" /></div><div data-testid="two"><FilePreview connectionKey="actor" fileId="notes" instanceKey="files" /></div></QueryClientProvider>);
  const one = within(screen.getByTestId("one")); const two = within(screen.getByTestId("two"));
  const first = await one.findByTitle("report.html"); const second = await two.findByTitle("report.html");
  fireEvent.click(one.getByRole("button", { name: "Refresh webpage" }));
  await waitFor(() => expect(one.getByTitle("report.html")).not.toBe(first));
  expect(two.getByTitle("report.html")).toBe(second);
  expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(second.getAttribute("src"));
});

it("offers download without eagerly reading an oversized HTML document", async () => {
  api.read.mockResolvedValue({ file: { ...file, originalFilename: "large.html", mimeType: "text/html", sizeBytes: 11 * 1024 * 1024 } });
  setup();
  expect(await screen.findByText("Files larger than 10 MB can be downloaded to view.")).toHaveAttribute("role", "status");
  expect(api.content).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Download file" })).toBeEnabled();
});
