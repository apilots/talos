import { describe, expect, it, vi } from "vitest";

import type { HttpAdapter } from "../adapters";
import { TalosClient } from "../client";

describe("TalosClient stream termination", () => {
  it("throws explicit termination errors for lagged timeline streams", async () => {
    const http: HttpAdapter = { request: vi.fn() };
    const stream = vi.fn().mockImplementation(
      async (_url, _options, onMessage: (data: string) => void) => {
        onMessage(JSON.stringify({
          reason: "lagged",
          message: "subscription lagged behind retained live events",
        }));
      },
    );
    const client = new TalosClient(http, { stream }, "http://localhost:8080");

    await expect(client.turns.events("session-1", undefined, vi.fn())).rejects.toMatchObject({
      name: "TalosStreamTerminatedError",
      reason: "lagged",
      message: "subscription lagged behind retained live events",
    });
  });

  it("ignores completed timeline subscription notifications", async () => {
    const http: HttpAdapter = { request: vi.fn() };
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(
      async (_url, _options, onMessage: (data: string) => void) => {
        onMessage(JSON.stringify({
          reason: "completed",
          message: "event subscription completed",
        }));
      },
    );
    const client = new TalosClient(http, { stream }, "http://localhost:8080");

    await expect(client.turns.events("session-1", undefined, onEvent)).resolves.toBeUndefined();
    expect(onEvent).not.toHaveBeenCalled();
  });
});
