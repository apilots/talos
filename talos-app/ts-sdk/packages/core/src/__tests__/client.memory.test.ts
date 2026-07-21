import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";

describe("TalosClient memory API", () => {
  it("serializes memory search filters on the request URL", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = { stream: vi.fn().mockResolvedValue(undefined) };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.search({
      query: "hello",
      limit: 5,
      kinds: ["fact", "decision"],
      sinceAt: 1,
      untilAt: 2,
    });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?query=hello&limit=5&kinds=fact%2Cdecision&sinceAt=1&untilAt=2",
    );
  });

  it("forwards abort signals to memory search requests", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = { stream: vi.fn().mockResolvedValue(undefined) };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");
    const controller = new AbortController();

    await client.memory.search({ query: "hello" }, { signal: controller.signal });

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/memory/search?query=hello",
      { signal: controller.signal },
    );
  });

  it("gets a memory entry by stable id", async () => {
    const request = vi.fn().mockResolvedValue({ memory: { memoryId: "mem-1" } });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.memory.getEntry({ memoryId: "mem-1" });

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/memory/entries/mem-1");
  });
});
