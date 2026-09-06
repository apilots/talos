import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient profile", () => {
  it("exposes profile reads through the desktop bridge", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(async function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
      calls.push({ method, params });
      if (method === "profile/read") {
        return {
          sessionCount: 1,
          messageCount: 2,
          projectCount: 3,
          usage: {
            input_tokens: 4,
            output_tokens: 5,
            total_tokens: 9,
            cache_creation_input_tokens: null,
            cache_read_input_tokens: null,
          },
          usageAvailable: true,
          usageSessionCount: 1,
          usageStartedAt: null,
          modelUsage: [],
          dailyUsage: [],
        } as T;
      }
      throw new Error(`unexpected method: ${method}`);
    });
    const client = createDesktopTalosClient(bridge);

    await expect(client.profile.read()).resolves.toMatchObject({
      sessionCount: 1,
      usage: {
        total_tokens: 9,
      },
    });

    expect(calls).toEqual([{ method: "profile/read", params: {} }]);
  });
});

function bridgeWithInvoke(invoke: <T>(method: AppProtocol.ClientMethod, params?: unknown) => Promise<T>): DesktopRendererBridge {
  return {
    invoke: async <T>(method: AppProtocol.ClientMethod, params: unknown) => ({ status: "success", data: await invoke<T>(method, params) }),
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  };
}
