import { describe, expect, it, vi } from "vitest";

import type { HttpAdapter, StreamAdapter } from "../adapters";
import { TalosClient } from "../client";

describe("TalosClient profile", () => {
  it("reads Profile through the SDK resource API", async () => {
    const request = vi.fn().mockResolvedValue({});
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");

    await client.profile.read();

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/profile");
  });
});
