// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";

import { act, cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FileInfo, FileUploadOptions } from "@talos/sdk-browser";

import "@/shared/i18n/setup";
import {
  getComposerInput,
  renderComposer,
  renderControlledComposer,
} from "./ChatComposer.test-utils";

const fileApiMocks = vi.hoisted(() => ({
  upload: vi.fn(),
  list: vi.fn(),
}));

vi.mock("../api", () => ({
  createFsDirectory: vi.fn(),
  readFsDirectory: vi.fn(),
  suggestSkills: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({ files: fileApiMocks }),
}));

describe("ChatComposer attachments", () => {
  beforeEach(() => {
    fileApiMocks.upload.mockResolvedValue({
      file: fileInfo("file-image", "pasted.png", "image/png", 3),
    });
    fileApiMocks.list.mockResolvedValue({
      data: [fileInfo("file-existing", "existing.pdf", "application/pdf", 10)],
      nextCursor: null,
    });
  });

  afterEach(() => {
    cleanup();
    fileApiMocks.upload.mockReset();
    fileApiMocks.list.mockReset();
    vi.restoreAllMocks();
  });

  it("uploads a pasted image and sends an attachment-only input", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    renderControlledComposer({
      onSend,
      supportedAttachmentKinds: ["image"],
    });
    const image = new File(["png"], "pasted.png", { type: "image/png" });

    fireEvent.paste(getComposerInput(), { clipboardData: { files: [image] } });

    expect(await screen.findByText("pasted.png")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(onSend.mock.calls[0]?.[1].map((file: FileInfo) => file.fileId)).toEqual(["file-image"]);
  });

  it("uploads multiple files with independent progress and preserves selection order", async () => {
    const uploads = new Map<
      string,
      {
        options: FileUploadOptions;
        resolve: (response: { file: FileInfo }) => void;
      }
    >();
    fileApiMocks.upload.mockImplementation(
      (_request, data: File, options: FileUploadOptions) =>
        new Promise<{ file: FileInfo }>((resolve) => {
          uploads.set(data.name, { options, resolve });
        }),
    );
    const onSend = vi.fn().mockResolvedValue(undefined);
    const { container } = renderControlledComposer({
      onSend,
      supportedAttachmentKinds: ["file"],
    });
    const docx = new File(["docx"], "notes.docx", {
      type: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    });
    const csv = new File(["a,b"], "data.csv", { type: "text/csv" });

    fireEvent.click(screen.getByRole("button", { name: "Add attachment" }));
    fireEvent.click(screen.getByRole("button", { name: "Upload a file" }));
    const input = container.querySelector<HTMLInputElement>('input[type="file"]');
    expect(input).toHaveAttribute("multiple");
    fireEvent.change(input!, { target: { files: [docx, csv] } });

    expect(await screen.findByText("notes.docx")).toBeInTheDocument();
    expect(screen.getByText("data.csv")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();

    act(() => {
      uploads.get("notes.docx")?.options.onProgress?.({
        loadedBytes: 4,
        totalBytes: 8,
      });
    });
    expect(screen.getByRole("progressbar", { name: "Uploading notes.docx" })).toHaveAttribute(
      "aria-valuenow",
      "50",
    );

    await act(async () => {
      uploads.get("data.csv")?.resolve({
        file: fileInfo("file-csv", "data.csv", "text/csv", 3),
      });
      uploads.get("notes.docx")?.resolve({
        file: fileInfo("file-docx", "notes.docx", docx.type, 4),
      });
    });

    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(onSend.mock.calls[0]?.[1].map((file: FileInfo) => file.fileId)).toEqual([
      "file-docx",
      "file-csv",
    ]);
  });

  it("selects an existing compatible file without uploading it again", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    renderControlledComposer({
      initialDraft: "Summarize this",
      onSend,
      supportedAttachmentKinds: ["file"],
    });

    fireEvent.click(screen.getByRole("button", { name: "Add attachment" }));
    fireEvent.click(screen.getByRole("button", { name: "Choose an existing file" }));
    fireEvent.click(await screen.findByRole("button", { name: /existing\.pdf/i }));
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(onSend).toHaveBeenCalledTimes(1));
    expect(fileApiMocks.upload).not.toHaveBeenCalled();
    expect(onSend.mock.calls[0]?.[1][0]?.fileId).toBe("file-existing");
  });

  it("disables attachment controls when the model has no attachment input", () => {
    renderComposer();

    expect(screen.getByRole("button", { name: "Add attachment" })).toBeDisabled();
  });
});

function fileInfo(
  fileId: string,
  originalFilename: string,
  mimeType: string,
  sizeBytes: number,
): FileInfo {
  return {
    fileId,
    scope: { type: "personal" },
    originalFilename,
    mimeType,
    sizeBytes,
    tags: [],
    metadata: {},
    createdAt: 1,
    updatedAt: 1,
  };
}
