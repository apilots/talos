// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { resolvePreviewResourcePath, TalosApiError, type FilePreviewDescriptor, type FilePreviewSource } from "@talos/sdk-core";
import { FilePreview } from "./FilePreview";

const api = vi.hoisted(() => ({ prepare: vi.fn(), content: vi.fn(), readBytes: vi.fn(), close: vi.fn(), resourceUrl: vi.fn(), openResource: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ files: { preview: { ...api, resolvePath: resolvePreviewResourcePath } } }) }));
vi.mock("@/shared/lib/open-resource", async importOriginal => ({ ...await importOriginal<typeof import("@/shared/lib/open-resource")>(), openResource: api.openResource }));
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
afterEach(() => { cleanup(); vi.restoreAllMocks(); clients.splice(0).forEach(client => client.clear()); });

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

it("inspects the selected archive member through the shared read API before choosing its view", async () => {
  // jsdom has no text-range geometry; retain the real CodeMirror read flow.
  const createRange = document.createRange.bind(document);
  vi.spyOn(document, "createRange").mockImplementation(() => Object.assign(createRange(), { getClientRects: () => [], getBoundingClientRect: () => new DOMRect() }));
  const metadata = { name: "source.ts", mimeType: "video/mp2t", sizeBytes: 24 };
  const resourcePath = "__preview__/entries/docs/source.ts";
  const archive = { name: "source-bundle.tar", mimeType: "application/x-tar", sizeBytes: 10240 };
  api.prepare.mockResolvedValue({ ...descriptor, metadata: archive, document: { kind: "archive", entries: [{ path: "./docs/source.ts", resourcePath, metadata }] } });
  api.readBytes.mockImplementation(async ({ offset, limit }: { offset: number; limit: number }) => ({ metadata: { ...metadata, mimeType: "text/plain" }, offset, data: limit ? new TextEncoder().encode("export const value = 42;") : new Uint8Array() }));
  const view = setup();
  fireEvent.click(await screen.findByRole("button", { name: /docs\/source.ts/ }));
  expect(await screen.findByRole("combobox", { name: "Encoding" })).toBeInTheDocument();
  await waitFor(() => expect(view.container.querySelector(".cm-content")).toHaveTextContent("export const value = 42;"));
  expect(api.readBytes).toHaveBeenCalledWith({ previewId: "preview", resourcePath, offset: 0, limit: 0 });
  expect(view.container.querySelector("iframe")).toBeNull();
  expect(screen.getByRole("navigation", { name: "Archive navigation" })).toHaveTextContent("source-bundle.tar");
  expect(screen.getByRole("navigation", { name: "Archive navigation" })).toHaveTextContent("./docs/source.ts");
  expect(screen.getByRole("textbox", { name: "File location" })).toHaveValue("Files / source-bundle.tar / ./docs/source.ts");
  fireEvent.click(screen.getByRole("button", { name: "Back to archive" }));
  expect(await screen.findByRole("heading", { name: "source-bundle.tar" })).toBeInTheDocument();
  expect(screen.getByRole("list", { name: "Archive contents" })).toHaveTextContent("./docs/source.ts");
});
