import { beforeEach, describe, expect, it, vi } from "vitest";

import { createSession, updateSessionApprovalMode } from "./api";

const createMock = vi.fn();
const patchMock = vi.fn();

vi.mock("@/shared/api/client", () => ({
  getTalosClient: () => ({
    sessions: {
      create: createMock,
      patch: patchMock,
    },
  }),
}));

describe("chat api approval mode wiring", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("sends session approval mode in create request", async () => {
    createMock.mockResolvedValue({
      context: { session_id: "session-1" },
      conf: {
        tools: {
          approval_policy: "always",
        },
      },
    });

    const session = await createSession("user-1", "qwen-max", undefined, "needs_approval");

    expect(createMock).toHaveBeenCalledWith(
      expect.objectContaining({
        model: "qwen-max",
        conf: {
          tools: {
            approval_policy: "always",
          },
        },
      }),
    );
    expect(session.conf?.tools?.approval_policy).toBe("always");
  });

  it("maps skip approval mode to never policy", async () => {
    await updateSessionApprovalMode("session-2", "skip");

    expect(patchMock).toHaveBeenCalledWith("session-2", {
      conf: {
        tools: {
          approval_policy: "never",
        },
      },
    });
  });
});
