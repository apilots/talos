// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { useState } from "react";
import type { FilePreviewPosition } from "@/shared/store/workspace-window";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { resolvePreviewResourcePath, TalosApiError, type FilePreviewDescriptor, type FilePreviewSource } from "@talos/sdk-core";
import { FilePreview } from "./FilePreview";

const api = vi.hoisted(() => ({ prepare: vi.fn(), content: vi.fn(), readBytes: vi.fn(), close: vi.fn(), resourceUrl: vi.fn(), openResource: vi.fn(), download: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ files: { preview: { ...api, resolvePath: resolvePreviewResourcePath } } }) }));
vi.mock("@/shared/lib/open-resource", async importOriginal => ({ ...await importOriginal<typeof import("@/shared/lib/open-resource")>(), openResource: api.openResource }));
vi.mock("../hooks/useFileContent", () => ({ downloadFile: api.download }));
vi.mock("./PdfPreview", () => ({ default: () => <p>Converted document</p> }));
const descriptor: FilePreviewDescriptor = { previewId: "preview", entryPath: "Review.md", resourceBaseUrl: null, document: null,
  metadata: { name: "Review.md", mimeType: "text/markdown", sizeBytes: 20 }, expiresAt: 9999999999 };
const clients: QueryClient[] = [];
const source: FilePreviewSource = { kind: "file", fileId: "notes" };
function setup(resource = source) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } }); clients.push(client);
  const view = render(<QueryClientProvider client={client}><FilePreview source={resource} instanceKey="session-one" connectionKey="alice" /></QueryClientProvider>);
  return { ...view, client };
}
beforeEach(() => {
  vi.resetAllMocks();
  let count = 0;
  api.prepare.mockImplementation(async () => ({ ...descriptor, previewId: `preview-${++count}` }));
  api.content.mockResolvedValue(new TextEncoder().encode("# Product\n\n| Feature | Evidence |\n| --- | --- |\n| Browser | Real task |\n\n<script>unsafe()</script>"));
  api.close.mockResolvedValue({ closed: true });
  api.resourceUrl.mockImplementation((preview: FilePreviewDescriptor, path = preview.entryPath) => `talos-preview://${preview.previewId}/${path}`);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); clients.splice(0).forEach(client => client.clear()); });

it("renders through one prepared resource and releases its grant when the tab closes", async () => {
  const view = setup();
  expect(await screen.findByRole("heading", { name: "Product" })).toBeInTheDocument();
  expect(screen.getByRole("table")).toHaveTextContent("Real task");
  expect(view.container.querySelector("script")).toBeNull();
  expect(api.prepare).toHaveBeenCalledWith({ source });
  expect(api.content).toHaveBeenCalledWith(expect.objectContaining({ previewId: "preview-1" }), "Review.md");
  view.unmount();
  expect(api.close).toHaveBeenCalledWith({ previewId: "preview-1" });
});
it("uses the same resource contract for a session path", async () => {
  const session: FilePreviewSource = { kind: "sessionPath", sessionId: "session-1", path: "output/report.md" };
  setup(session);
  await screen.findByRole("heading", { name: "Product" });
  expect(screen.getByRole("textbox", { name: "File location" })).toHaveValue("output/report.md");
  expect(api.prepare).toHaveBeenCalledWith({ source: session });
});
it("uses the SDK URL for native HTML assets without eagerly copying their bytes", async () => {
  api.prepare.mockResolvedValue({ ...descriptor, metadata: { name: "report.html", mimeType: "text/html", sizeBytes: 12 * 1024 * 1024 } });
  setup();
  expect(await screen.findByTitle("report.html")).toHaveAttribute("src", "talos-preview://preview/Review.md");
  expect(screen.getByTitle("report.html")).toHaveAttribute("sandbox", "allow-scripts allow-forms");
  expect(api.content).not.toHaveBeenCalled();
});
it("does not reuse an old account's body after a failed new preparation", async () => {
  const view = setup(); await screen.findByRole("heading", { name: "Product" });
  api.prepare.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "Access denied.", localization: null }));
  view.rerender(<QueryClientProvider client={view.client}><FilePreview source={source} instanceKey="session-one" connectionKey="bob" /></QueryClientProvider>);
  expect(screen.queryByRole("heading", { name: "Product" })).not.toBeInTheDocument();
  expect(await screen.findByRole("alert")).toHaveTextContent("Access denied.");
  expect(api.content).toHaveBeenCalledTimes(1);
  expect(api.close).toHaveBeenCalledWith({ previewId: "preview-1" });
});
it("retries through preparation after permission failure", async () => {
  api.prepare.mockRejectedValueOnce(new TalosApiError({ code: "permissionDenied", message: "Access denied.", localization: null }));
  setup(); await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("heading", { name: "Product" })).toBeInTheDocument();
});
it("refreshes only the owning context when two contexts preview the same file", async () => {
  api.prepare.mockImplementation(async () => ({ ...descriptor, previewId: crypto.randomUUID(), metadata: { name: "report.html", mimeType: "text/html", sizeBytes: 20 } }));
  const client = new QueryClient(); clients.push(client);
  render(<QueryClientProvider client={client}><div data-testid="one"><FilePreview source={source} connectionKey="actor" instanceKey="one" /></div><div data-testid="two"><FilePreview source={source} connectionKey="actor" instanceKey="two" /></div></QueryClientProvider>);
  const one = within(screen.getByTestId("one")); const two = within(screen.getByTestId("two"));
  const first = await one.findByTitle("report.html"); const second = await two.findByTitle("report.html");
  fireEvent.click(one.getByRole("button", { name: "Refresh webpage" }));
  await waitFor(() => expect(one.getByTitle("report.html")).not.toBe(first));
  expect(two.getByTitle("report.html")).toBe(second);
});

it("resolves Markdown links and images from the prepared document and keeps registered images on preview", async () => {
  api.prepare.mockImplementation(async ({ source }: { source: FilePreviewSource }) => source.kind === "sessionPath"
    ? { ...descriptor, entryPath: "docs/report.md" }
    : { ...descriptor, previewId: "picture", entryPath: "attached.png", metadata: { name: "attached.png", mimeType: "image/png", sizeBytes: 4 } });
  api.content.mockResolvedValue(new TextEncoder().encode("[Next](next.md)\n\n![Chart](images/chart.png)\n\n![Attached](talos-file://image-1)"));
  setup({ kind: "sessionPath", sessionId: "session-1", path: "docs/report.md" });
  fireEvent.click(await screen.findByRole("link", { name: "Next" }));
  expect(api.openResource).toHaveBeenCalledWith({ kind: "sessionFile", sessionId: "session-1", path: "docs/next.md" });
  expect(screen.getByAltText("Chart")).toHaveAttribute("src", "talos-preview://preview/docs/images/chart.png");
  expect(await screen.findByAltText("Attached")).toHaveAttribute("src", "talos-preview://picture/attached.png");
  expect(api.prepare).toHaveBeenCalledWith({ source: { kind: "file", fileId: "image-1" } });
});

it("shows package metadata and downloads the original archive", async () => {
  const metadata = { name: "source-bundle.tar", mimeType: "application/x-tar", sizeBytes: 10240 };
  const bytes = new Uint8Array([1, 2, 3]);
  api.prepare.mockResolvedValue({ ...descriptor, entryPath: metadata.name, metadata, document: null });
  api.content.mockResolvedValue(bytes);
  setup();
  expect(await screen.findByRole("heading", { name: metadata.name })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("Download");
  expect(screen.getByText("application/x-tar · 10.0 KiB")).toBeInTheDocument();
  expect(api.content).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Download file" }));
  await waitFor(() => expect(api.content).toHaveBeenCalledWith(expect.objectContaining({ entryPath: metadata.name }), metadata.name));
  expect(api.download).toHaveBeenCalledWith(bytes, metadata.name, metadata.mimeType);
});

it("downloads the original Office source while displaying its derived PDF", async () => {
  const metadata = { name: "report.docx", mimeType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", sizeBytes: 2048 };
  const bytes = new Uint8Array([4, 5, 6]);
  api.prepare.mockResolvedValue({ ...descriptor, entryPath: metadata.name, metadata, document: { kind: "pdf", resourcePath: "__preview__/document.pdf" } });
  api.content.mockResolvedValue(bytes);
  setup();
  await screen.findByText("Converted document");
  fireEvent.click(screen.getByRole("button", { name: "Download file" }));
  await waitFor(() => expect(api.download).toHaveBeenCalledWith(bytes, metadata.name, metadata.mimeType));
  expect(api.content).toHaveBeenCalledWith(expect.objectContaining({ entryPath: metadata.name }), metadata.name);
});

it("switches HTML between browser and shared source view within one resource grant", async () => {
  const createRange = document.createRange.bind(document);
  vi.spyOn(document, "createRange").mockImplementation(() => Object.assign(createRange(), { getClientRects: () => [], getBoundingClientRect: () => new DOMRect() }));
  const body = new TextEncoder().encode("<h1>HTML source</h1>");
  api.prepare.mockResolvedValue({ ...descriptor, entryPath: "report.html", metadata: { name: "report.html", mimeType: "text/html", sizeBytes: body.byteLength } });
  api.readBytes.mockResolvedValue({ offset: 0, metadata: { sizeBytes: body.byteLength }, data: body });
  const client = new QueryClient(); clients.push(client);
  function Controlled() {
    const [position, setPosition] = useState<FilePreviewPosition>();
    return <FilePreview source={source} connectionKey="alice" instanceKey="html" position={position} onPositionChange={setPosition} />;
  }
  const view = render(<QueryClientProvider client={client}><Controlled /></QueryClientProvider>);
  await screen.findByTitle("report.html");
  fireEvent.click(screen.getByRole("button", { name: "Source" }));
  await waitFor(() => expect(view.container.querySelector(".cm-content")).toHaveTextContent("<h1>HTML source</h1>"));
  fireEvent.click(screen.getByRole("button", { name: "Preview" }));
  expect(await screen.findByTitle("report.html")).toBeInTheDocument();
  expect(api.prepare).toHaveBeenCalledTimes(1);
  expect(api.readBytes).toHaveBeenCalledWith({ previewId: "preview", resourcePath: "report.html", offset: 0, limit: 1024 * 1024 });
});
