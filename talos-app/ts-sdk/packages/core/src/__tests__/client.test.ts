import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";

describe("TalosClient", () => {
  it("uses absolute urls for http requests", async () => {
    const request = vi.fn().mockResolvedValue({ data: [] });
    const http: HttpAdapter = { request };
    const streamAdapter: StreamAdapter = {
      stream: vi.fn().mockResolvedValue(undefined),
    };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080/");

    await client.models.list();

    expect(request).toHaveBeenCalledWith("http://localhost:8080/v1/models");
  });

  it("forces stream=true for streaming messages", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const stream = vi.fn().mockResolvedValue(undefined);
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.messages.sendStream(
      "session-1",
      {
        input: [{ type: "text", text: "hello" }],
      },
      () => undefined,
    );

    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/messages",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          input: [{ type: "text", text: "hello" }],
          stream: true,
        }),
      }),
      expect.any(Function),
    );
  });
});
