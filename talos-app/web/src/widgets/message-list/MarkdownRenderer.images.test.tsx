// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import "@/shared/i18n/setup";
import { TalosApiError } from "@talos/sdk-browser";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MarkdownRenderer } from "./MarkdownRenderer";

const content = vi.hoisted(() => vi.fn());
vi.mock("@/shared/api/client", () => ({ getTalosClient: () => ({ files: { content } }) }));
const create = vi.fn();
const revoke = vi.fn();
beforeEach(() => {
  create.mockReturnValue("blob:authorized-image");
  vi.stubGlobal("URL", class extends URL {
    static createObjectURL = create;
    static revokeObjectURL = revoke;
  });
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
    close: { configurable: true, value(this: HTMLDialogElement) { this.open = false; } },
  });
  content.mockResolvedValue({ file: { mimeType: "image/png" }, data: new Uint8Array([1, 2, 3]) });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
  Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
  Reflect.deleteProperty(HTMLDialogElement.prototype, "close");
});

it("loads explicit final-answer File references through authorization, previews and reloads them", async () => {
  const text = "Website analysis\n\n![Feature screenshot](talos-file://screenshot-1)";
  const first = render(<MarkdownRenderer className="markdown-body" text={text} />);
  const button = await screen.findByRole("button", { name: "Preview Feature screenshot" });
  expect(content).toHaveBeenCalledWith({ fileId: "screenshot-1" });
  expect(within(button).getByRole("img")).toHaveAttribute("src", "blob:authorized-image");
  fireEvent.click(button);
  expect(within(screen.getByRole("dialog")).getByRole("img")).toHaveAttribute("src", "blob:authorized-image");
  first.unmount();
  expect(revoke).toHaveBeenCalledWith("blob:authorized-image");
  render(<MarkdownRenderer className="markdown-body" text={text} />);
  await screen.findByRole("button", { name: "Preview Feature screenshot" });
  expect(content).toHaveBeenCalledTimes(2);
});

it("shows denied File access without creating an image URL", async () => {
  content.mockRejectedValue(new TalosApiError({ code: "permissionDenied", message: "You cannot access this file", localization: null }));
  render(<MarkdownRenderer className="markdown-body" text="![Private](talos-file://private-image)" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("You cannot access this file");
  expect(create).not.toHaveBeenCalled();
  expect(screen.queryByRole("img")).not.toBeInTheDocument();
});

it("does not treat bare IDs, paths, links or malformed references as authorized File images", () => {
  const { container } = render(<MarkdownRenderer className="markdown-body" text={'![Bare](screenshot-1)\n\n![Relative](./photo.png)\n\n![Remote](https://example.com/photo.png)\n\n![Bad](talos-file://id/path)\n\n![Unsafe](javascript:alert)\n\n[Link](talos-file://private-image)'} />);
  expect(content).not.toHaveBeenCalled();
  expect(screen.getByAltText("Bare")).toHaveAttribute("src", "screenshot-1");
  expect(screen.getByAltText("Relative")).toHaveAttribute("src", "./photo.png");
  expect(screen.getByAltText("Remote")).toHaveAttribute("src", "https://example.com/photo.png");
  expect(screen.getByAltText("Bad")).not.toHaveAttribute("src");
  expect(screen.getByAltText("Unsafe")).not.toHaveAttribute("src");
  expect(container.querySelector("a")).toHaveAttribute("href", "talos-file://private-image");
});

it("releases a pending File request without publishing a URL after unmount", async () => {
  let resolve!: (value: unknown) => void;
  content.mockReturnValue(new Promise(done => { resolve = done; }));
  const view = render(<MarkdownRenderer className="markdown-body" text="![Pending](talos-file://pending)" />);
  view.unmount();
  resolve({ file: { mimeType: "image/png" }, data: new Uint8Array([1]) });
  await Promise.resolve();
  expect(create).not.toHaveBeenCalled();
});
