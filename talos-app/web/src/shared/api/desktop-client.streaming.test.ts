import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient streaming", () => {
  it.each([
    {
      name: "closes the default session event subscription when idle or terminal",
      options: undefined,
      closeWhenIdle: true,
      closeOnTerminal: true,
    },
    {
      name: "keeps an explicitly persistent session event subscription open across turns",
      options: { persistent: true },
      closeWhenIdle: false,
      closeOnTerminal: false,
    },
  ])("$name", async ({ options, closeWhenIdle, closeOnTerminal }) => {
    let subscribeParams: AppProtocol.SubscribeTimelineRequest | undefined;
    const controller = new AbortController();
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
        if (method === "event/subscribe") {
          subscribeParams = params as AppProtocol.SubscribeTimelineRequest;
          queueMicrotask(() => controller.abort());
          return { subscriptionId: "sub-session" } as T;
        }
        if (method === "event/unsubscribe") {
          return { unsubscribed: true } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.turns.events(
      "session-1",
      undefined,
      () => undefined,
      controller.signal,
      options,
    );

    expect(subscribeParams).toMatchObject({
      sessionId: "session-1",
      turnId: null,
      closeWhenIdle,
      closeOnTerminal,
    });
  });
});
