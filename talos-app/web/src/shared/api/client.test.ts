import { afterEach, describe, expect, it, vi } from "vitest";

import { bindComputerTarget, deferProductClient } from "./computer-target";

import {
  getTalosClient,
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "./client";

describe("Talos client runtime factory", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("allows desktop runtime to inject a transport-specific client", () => {
    const client = { marker: "desktop" } as unknown as TalosWebClient;
    const factory = vi.fn(() => client);

    setTalosClientFactory(factory);

    expect(getTalosClient()).toBe(client);
    expect(factory).toHaveBeenCalledTimes(1);
  });
});


it("binds capabilities only when used and keeps File and preview identities stable", async () => {
  const prepare = vi.fn().mockResolvedValue({});
  const readFiles = vi.fn();
  const readPreview = vi.fn();
  const listModels = vi.fn().mockResolvedValue({ models: [] });
  const client = {
    models: { list: listModels },
    get files() {
      readFiles();
      return { get preview() { readPreview(); return { prepare }; } };
    },
  } as unknown as TalosWebClient;
  const resolve = vi.fn().mockResolvedValue(client);
  const bound = bindComputerTarget(client, "worker-b", resolve);
  expect(readFiles).not.toHaveBeenCalled();
  await bound.models.list();
  expect(listModels).toHaveBeenCalledWith({ nodeId: "worker-b" });
  expect(readFiles).not.toHaveBeenCalled();
  const files = bound.files;
  expect(bound.files).toBe(files);
  expect(readPreview).not.toHaveBeenCalled();
  const preview = files.preview;
  expect(files.preview).toBe(preview);
  expect(readPreview).toHaveBeenCalledTimes(1);
  await preview.prepare({ source: { kind: "file", fileId: "document" } });
  expect(prepare).toHaveBeenLastCalledWith({ source: { kind: "file", fileId: "document", nodeId: "worker-b" } });
  await preview.prepare({ source: { kind: "file", fileId: "document", nodeId: "worker-c" } });
  expect(resolve).toHaveBeenLastCalledWith("worker-c");
  expect(prepare).toHaveBeenLastCalledWith({ source: { kind: "file", fileId: "document", nodeId: "worker-c" } });
  resolve.mockClear();
  const source = { kind: "sessionPath", sessionId: "saved-session", path: "report.docx" } as const;
  await preview.prepare({ source });
  expect(prepare).toHaveBeenLastCalledWith({ source });
  expect(resolve).not.toHaveBeenCalled();
});

it("does not inspect unused local capabilities when binding a deferred product client", async () => {
  const readFiles = vi.fn(() => { throw new Error("Files have not been requested."); });
  const local = { models: { list: vi.fn() }, get files() { return readFiles(); } } as unknown as TalosWebClient;
  const listModels = vi.fn().mockResolvedValue({ models: [] });
  const resolve = vi.fn().mockResolvedValue({ models: { list: listModels } });
  const deferred = deferProductClient(local, resolve);
  const bound = bindComputerTarget(deferred, "worker-b");
  expect(readFiles).not.toHaveBeenCalled();
  expect(resolve).not.toHaveBeenCalled();
  await bound.models.list();
  expect(listModels).toHaveBeenCalledWith({ nodeId: "worker-b" });
  expect(resolve).toHaveBeenCalledTimes(1);
  expect(readFiles).not.toHaveBeenCalled();
});
