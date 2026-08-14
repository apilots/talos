import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";

describe("TalosClient user actions", () => {
  it("submits typed user actions through the canonical HTTP route", async () => {
    const request = vi.fn().mockResolvedValue({
      actionId: "action-1",
      state: "responseAccepted",
    });
    const client = new TalosClient(
      { request },
      { stream: vi.fn().mockResolvedValue(undefined) },
      "http://localhost:8080",
    );
    const body = {
      sessionId: "session-1",
      turnId: "turn-1",
      actionId: "action-1",
      response: { type: "sandboxApproval" as const, response: "allowOnce" as const },
    };

    await client.userActions.submit(body);

    expect(request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/user-actions/submit",
      { method: "POST", body: JSON.stringify(body) },
    );
  });
});
