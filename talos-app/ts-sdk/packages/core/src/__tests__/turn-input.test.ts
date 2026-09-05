import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";

describe("TalosClient Turn input", () => {
  it("submits a typed UserAction response through the canonical Turn route", async () => {
    const request = vi.fn().mockResolvedValue({
      requestId: "request-1",
      identity: { sessionId: "session-1", turnId: "turn-1" },
      messageId: "message-1",
    });
    const client = new TalosClient(
      { request },
      { stream: vi.fn().mockResolvedValue(undefined) },
      "http://localhost:8080",
    );
    const body = {
      identity: { sessionId: "session-1", turnId: "turn-1" },
      input: {
        type: "userActionResponse" as const,
        actionId: "action-1",
        response: { type: "sandboxApproval" as const, response: "allowOnce" as const },
      },
      clientRequestId: null,
    };

    await client.turns.input(body);

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/turns/input",
      { method: "POST", body: JSON.stringify(body) },
    );
  });
});
