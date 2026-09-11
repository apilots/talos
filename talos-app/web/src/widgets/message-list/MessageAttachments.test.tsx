// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { TalosApiError } from "@talos/sdk-browser";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { TextMessageBlock } from "./TextMessageBlock";

const files = vi.hoisted(() => ({ read: vi.fn(), content: vi.fn() }));
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ files }) }));

it("replays mixed attachments, previews the selected image, and releases loaded image URLs", async () => {
  // jsdom lacks native dialog methods; focus and Escape are verified in the browser.
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
    close: { configurable: true, value(this: HTMLDialogElement) { this.open = false; } },
  });
  const create = vi.fn().mockReturnValueOnce("blob:first").mockReturnValueOnce("blob:second");
  const revoke = vi.fn();
  vi.stubGlobal("URL", class extends URL {
    static createObjectURL = create;
    static revokeObjectURL = revoke;
  });
  files.read.mockImplementation(async ({ fileId }: { fileId: string }) => {
    if (fileId === "missing") throw new TalosApiError({ code: "internalError", message: "Attachment could not be loaded", localization: null });
    return { file: { fileId, originalFilename: `${fileId}.${fileId === "notes" ? "txt" : "png"}`, mimeType: fileId === "notes" ? "text/plain" : "image/png" } };
  });
  files.content.mockResolvedValue({ file: { mimeType: "image/png" }, data: new Uint8Array([1, 2, 3]) });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const view = render(<QueryClientProvider client={queryClient}><TextMessageBlock fileIds={["first", "second", "notes", "missing"]} kind="user" text="Identify the images" /></QueryClientProvider>);
  try {
    await screen.findByRole("button", { name: "Preview first.png" });
    fireEvent.click(await screen.findByRole("button", { name: "Preview second.png" }));
    const dialog = screen.getByRole("dialog", { name: "Preview second.png" });
    expect(within(dialog).getByRole("img")).toHaveAttribute("src", "blob:second");
    fireEvent.click(within(dialog).getByRole("button", { name: "Close image preview" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getAllByRole("img")).toHaveLength(2);
    expect(screen.getByText("notes.txt")).toBeInTheDocument();
    expect(await screen.findByRole("alert")).toHaveTextContent("Attachment could not be loaded");
    expect(files.content.mock.calls.map(([request]) => request.fileId)).toEqual(["first", "second"]);
    fireEvent.error(within(screen.getByRole("button", { name: "Preview first.png" })).getByRole("img"));
    expect(screen.queryByRole("button", { name: "Preview first.png" })).not.toBeInTheDocument();
    expect(screen.getAllByRole("alert").map((alert) => alert.textContent)).toEqual(["Attachment could not be loaded", "Attachment could not be loaded"]);
    expect(screen.getByRole("button", { name: "Preview second.png" })).toBeInTheDocument();
  } finally {
    view.unmount();
    queryClient.clear();
    expect(revoke.mock.calls.flat()).toEqual(["blob:first", "blob:second"]);
    vi.unstubAllGlobals();
    Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
    Reflect.deleteProperty(HTMLDialogElement.prototype, "close");
  }
});
