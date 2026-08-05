import { describe, expect, it, vi } from "vitest";

import type { HttpAdapter, StreamAdapter } from "../adapters";
import { TalosClient } from "../client";

describe("TalosClient config routes", () => {
  it("reads and writes shared config through the config route", async () => {
    const request = vi.fn().mockResolvedValue({ type: "webSearch", config: {} });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");
    const config = { type: "webSearch", value: {} } as never;

    await client.config.get("webSearch");
    await client.config.update(config);

    expect(request).toHaveBeenNthCalledWith(1, "http://localhost:8080/v1/config/webSearch");
    expect(request).toHaveBeenNthCalledWith(2, "http://localhost:8080/v1/config", {
      method: "PUT",
      body: JSON.stringify(config),
    });
  });
});
