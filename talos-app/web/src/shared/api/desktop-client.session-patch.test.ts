import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

function bridgeWithInvoke(
  invoke: <T>(method: AppProtocol.ClientMethod, params?: unknown) => Promise<T>,
): DesktopRendererBridge {
  return {
    invoke: async <T>(method: AppProtocol.ClientMethod, params: unknown) => ({
      status: "success",
      data: await invoke<T>(method, params),
    }),
    onEvent: () => () => undefined,
    onSubscriptionTerminated: () => () => undefined,
  };
}

describe("createDesktopTalosClient session patch", () => {
  it("forwards pinned patches through sessions.patch", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge = bridgeWithInvoke(function <T>(
      method: AppProtocol.ClientMethod,
      params?: unknown,
    ): Promise<T> {
      calls.push({ method, params });
      return Promise.resolve({ session: { id: "session-1", pinned: true } } as T);
    });
    const client = createDesktopTalosClient(bridge);

    await client.sessions.patch({
      sessionId: "session-1",
      conf: null,
      currentDirectory: null,
      pinned: true,
    });

    expect(calls).toEqual([
      {
        method: "session/patch",
        params: {
          sessionId: "session-1",
          conf: null,
          currentDirectory: null,
          pinned: true,
        },
      },
    ]);
  });
});
