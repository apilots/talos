import { expect, it, vi } from "vitest";
import { TalosClient } from "../client";
import { AppServerClient } from "../app-server";
import type { SyncMetricsResponse } from "../types";
import { RecordingTransport } from "./app-server.fixture";

it("exposes account metrics through both SDK transports without account selectors or browser Sync credentials", async () => {
  const metrics: SyncMetricsResponse = {
    endpoint: "https://sync.example",
    controlPlanePeerId: null,
    observedAt: 1_700_000_000,
    lastFlushedAt: null,
    complete: false,
    metadata: { computers: 0, projects: 0, sessions: 0, files: 0 },
    peers: [],
  };
  const request = vi.fn().mockResolvedValue(metrics);
  const browser = new TalosClient(
    { request },
    { stream: vi.fn() },
    "http://localhost:8080/",
  );
  await expect(browser.sync.metrics()).resolves.toEqual(metrics);
  expect(request).toHaveBeenCalledExactlyOnceWith(
    "http://localhost:8080/v1/sync/metrics",
  );
  const transport = new RecordingTransport();
  const rpc = vi.spyOn(transport, "request").mockResolvedValue(metrics);
  const desktop = new AppServerClient(transport);
  await expect(desktop.sync.metrics()).resolves.toEqual(metrics);
  expect(rpc).toHaveBeenCalledExactlyOnceWith("sync/metrics", null);
  const failure = {
    code: "unauthenticated",
    message: "Sign in before reading metrics.",
    localization: null,
  };
  request.mockRejectedValueOnce(failure);
  await expect(browser.sync.metrics()).rejects.toBe(failure);
});
