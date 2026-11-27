// @vitest-environment jsdom

import { createElement, type PropsWithChildren } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, renderHook, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileInfo, FileUploadOptions } from "@talos/sdk-browser";

import { AttachmentSessionContext } from "@/shared/api/attachment-session";
import { useComposerAttachments } from "./useComposerAttachments";
import { ComposerAttachmentTray } from "./ComposerAttachmentTray";

const fileApi = vi.hoisted(() => ({
  upload: vi.fn(),
  list: vi.fn(),
  selectClient: vi.fn(),
}));

vi.mock("@/shared/api/client", () => ({
  getTalosClient: (owner?: unknown) => { fileApi.selectClient(owner); return { files: fileApi }; },
}));

const PERSONAL_IMAGE: FileInfo = {
  fileId: "personal-image",
  scope: { type: "personal" },
  originalFilename: "image.png",
  mimeType: "image/png",
  sizeBytes: 8,
  tags: [],
  metadata: {},
  createdAt: 1,
  updatedAt: 1,
};

const PROJECT_FILE: FileInfo = {
  ...PERSONAL_IMAGE,
  fileId: "project-file",
  scope: { type: "project", projectId: "project-1" },
  originalFilename: "notes.pdf",
  mimeType: "application/pdf",
};

function createWrapper(sessionId?: string) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return function Wrapper({ children }: PropsWithChildren) {
    return createElement(QueryClientProvider, { client: queryClient },
      createElement(AttachmentSessionContext.Provider, { value: sessionId }, children));
  };
}

describe("useComposerAttachments", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    fileApi.upload.mockResolvedValue({ file: PERSONAL_IMAGE });
    fileApi.list.mockResolvedValue({
      data: [
        PERSONAL_IMAGE,
        PROJECT_FILE,
        {
          ...PROJECT_FILE,
          fileId: "other-project-file",
          scope: { type: "project", projectId: "project-2" },
        },
      ],
      nextCursor: null,
    });
  });

  it("uploads into the explicit composer scope and keeps ordered file ids", async () => {
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "project", projectId: "project-1" },
          supportedKinds: ["image", "file"],
        }),
      { wrapper: createWrapper("talo-chat") },
    );
    act(() => result.current.select(PROJECT_FILE));
    const image = new File(["png"], "pasted.png", { type: "image/png" });

    await act(async () => result.current.upload([image]));

    expect(fileApi.upload).toHaveBeenCalledWith(
      {
        filename: "pasted.png",
        scope: { type: "project", projectId: "project-1" },
        tags: [],
        metadata: {},
      },
      image,
      {
        signal: expect.any(AbortSignal) as unknown,
        onProgress: expect.any(Function) as unknown,
      },
    );
    expect(fileApi.selectClient).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "talo-chat" }));
    expect(result.current.fileIds).toEqual(["project-file", "personal-image"]);
  });

  it("tracks progress and turns the same draft item into a ready attachment", async () => {
    let options: FileUploadOptions | undefined;
    let resolveUpload: ((value: { file: FileInfo }) => void) | undefined;
    fileApi.upload.mockImplementation(
      (_request, _data, uploadOptions: FileUploadOptions) =>
        new Promise<{ file: FileInfo }>((resolve) => {
          options = uploadOptions;
          resolveUpload = resolve;
        }),
    );
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "personal" },
          supportedKinds: ["image"],
        }),
      { wrapper: createWrapper() },
    );
    const image = new File(["png"], "pasted.png", { type: "image/png" });
    let pending: Promise<void>;

    act(() => {
      pending = result.current.upload([image]);
    });
    const itemId = result.current.items[0]?.id;
    expect(result.current.items).toEqual([
      expect.objectContaining({ id: itemId, status: "uploading", progress: null }) as unknown,
    ]);

    await waitFor(() => expect(options?.onProgress).toBeDefined());
    act(() => options?.onProgress?.({ loadedBytes: 5, totalBytes: 10 }));
    expect(result.current.items[0]).toEqual(
      expect.objectContaining({
        id: itemId,
        status: "uploading",
        progress: { loadedBytes: 5, totalBytes: 10 },
      }),
    );

    await act(async () => {
      resolveUpload?.({ file: PERSONAL_IMAGE });
      await pending;
    });
    expect(result.current.items).toEqual([
      expect.objectContaining({ id: itemId, status: "ready", file: PERSONAL_IMAGE }) as unknown,
    ]);
  });

  it("displays consumed upload bytes while the total remains unknown", async () => {
    let options: FileUploadOptions | undefined;
    fileApi.upload.mockImplementation((_request, _data, uploadOptions: FileUploadOptions) => {
      options = uploadOptions;
      return new Promise(() => {});
    });
    function Harness() {
      const attachments = useComposerAttachments({ scope: { type: "personal" }, supportedKinds: ["image"] });
      return <>
        <button onClick={() => void attachments.upload([new File(["png"], "pasted.png", { type: "image/png" })])}>Upload</button>
        <ComposerAttachmentTray disabled={false} error={attachments.error} items={attachments.items} onRemove={attachments.remove} />
      </>;
    }
    const view = render(<Harness />, { wrapper: createWrapper() });
    fireEvent.click(screen.getByRole("button", { name: "Upload" }));
    await waitFor(() => expect(options?.onProgress).toBeDefined());

    act(() => options?.onProgress?.({ loadedBytes: 2048, totalBytes: null }));

    expect(screen.getByText("· 2.0 KiB")).toBeTruthy();
    expect(screen.getByRole("progressbar").getAttribute("aria-valuenow")).toBeNull();
    view.unmount();
  });

  it("cancels an uploading item when it is removed", async () => {
    let signal: AbortSignal | undefined;
    fileApi.upload.mockImplementation(
      (_request, _data, options: FileUploadOptions) =>
        new Promise((_resolve, reject) => {
          signal = options.signal;
          options.signal?.addEventListener(
            "abort",
            () => reject(new DOMException("aborted", "AbortError")),
            { once: true },
          );
        }),
    );
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "personal" },
          supportedKinds: ["image"],
        }),
      { wrapper: createWrapper() },
    );
    const image = new File(["png"], "pasted.png", { type: "image/png" });
    let pending: Promise<void>;

    act(() => {
      pending = result.current.upload([image]);
    });
    await waitFor(() => expect(signal).toBeDefined());
    act(() => result.current.remove(result.current.items[0].id));
    await act(async () => pending);

    expect(signal?.aborted).toBe(true);
    expect(result.current.items).toEqual([]);
  });

  it("keeps a failed upload visible and blocks attachment readiness", async () => {
    fileApi.upload.mockRejectedValue(new Error("network lost"));
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "personal" },
          supportedKinds: ["file"],
        }),
      { wrapper: createWrapper() },
    );

    await act(async () =>
      result.current.upload([
        new File(["csv"], "data.csv", { type: "text/csv" }),
      ]),
    );

    expect(result.current.items).toEqual([
      expect.objectContaining({ status: "failed", error: {
        code: "internalError", message: "network lost", localization: null,
      } }) as unknown,
    ]);
    expect(result.current.hasFailedUploads).toBe(true);
    expect(result.current.fileIds).toEqual([]);
  });

  it("lists only selectable files for the active scope and model", async () => {
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "project", projectId: "project-1" },
          supportedKinds: ["image"],
        }),
      { wrapper: createWrapper() },
    );

    await act(async () => result.current.loadAvailable());

    expect(fileApi.list).toHaveBeenCalledWith({ limit: 100 });
    expect(result.current.availableFiles).toEqual([PERSONAL_IMAGE]);
  });

  it("loads additional selectable files with the server cursor", async () => {
    fileApi.list
      .mockResolvedValueOnce({
        data: [PERSONAL_IMAGE],
        nextCursor: "next-page",
      })
      .mockResolvedValueOnce({
        data: [PROJECT_FILE],
        nextCursor: null,
      });
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "project", projectId: "project-1" },
          supportedKinds: ["image", "file"],
        }),
      { wrapper: createWrapper("talo-chat") },
    );

    await act(async () => result.current.loadAvailable());
    expect(fileApi.list).toHaveBeenNthCalledWith(1, { limit: 100 });
    expect(result.current.hasMoreAvailable).toBe(true);

    await act(async () => result.current.loadMoreAvailable());

    expect(fileApi.list).toHaveBeenNthCalledWith(2, {
      cursor: "next-page",
      limit: 100,
    });
    expect(result.current.availableFiles).toEqual([PERSONAL_IMAGE, PROJECT_FILE]);
    expect(result.current.hasMoreAvailable).toBe(false);
  });

  it("deduplicates selected files and removes them by id", () => {
    const { result } = renderHook(
      () =>
        useComposerAttachments({
          scope: { type: "personal" },
          supportedKinds: ["image", "file"],
        }),
      { wrapper: createWrapper() },
    );

    act(() => {
      result.current.select(PERSONAL_IMAGE);
      result.current.select(PERSONAL_IMAGE);
    });
    expect(result.current.fileIds).toEqual([PERSONAL_IMAGE.fileId]);

    act(() => result.current.remove(PERSONAL_IMAGE.fileId));
    expect(result.current.fileIds).toEqual([]);
  });
});
