import { describe, expect, it, vi } from "vitest";
import { TalosStreamTerminatedError } from "@talos/sdk-core";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient streaming", () => {
  it.each(["callback", "lagged", "interrupted"] as const)("rejects a subscription for %s without losing the failure", async (failure) => {
    let eventHandler: ((event: AppProtocol.TimelineEvent) => void) | undefined;
    let terminatedHandler: ((event: AppProtocol.SubscriptionTerminatedNotification) => void) | undefined;
    const cleanup = vi.fn();
    const invoke = vi.fn();
    const bridge: DesktopRendererBridge = {
      invoke: async <T,>(...args: Parameters<DesktopRendererBridge["invoke"]>) => {
        invoke(...args);
        return { status: "success", data: { subscriptionId: "sub-1" } as T };
      },
      onEvent: (handler) => { eventHandler = handler; return cleanup; },
      onSubscriptionTerminated: (handler) => { terminatedHandler = handler; return cleanup; },
    };
    const callbackError = new Error("Invalid terminal event.");
    const operation = createDesktopTalosClient(bridge).turns.events("session-1", undefined, () => { throw callbackError; });
    const assertion = failure === "callback"
      ? expect(operation).rejects.toBe(callbackError)
      : expect(operation).rejects.toBeInstanceOf(TalosStreamTerminatedError);
    await Promise.resolve();
    if (failure === "callback") {
      eventHandler?.({ eventId: "ev_1", eventSeq: 1, persistedAt: 1, event: { type: "agent", data: { identity: { sessionId: "session-1", turnId: "turn-1" }, sequence: 1, occurredAt: 1, payload: { type: "turnStarted", data: {} } } } });
    } else {
      terminatedHandler?.({ subscriptionId: "sub-1", sessionId: "session-1", reason: failure, message: "Replay is required." });
    }
    await assertion;
    expect(cleanup).toHaveBeenCalledTimes(2);
    expect(invoke.mock.calls).toContainEqual(["event/unsubscribe", { subscriptionId: "sub-1" }, { actorCredentials: null }]);
  });

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
      invoke: async function <T>(method: AppProtocol.ClientMethod, params: unknown): Promise<AppProtocol.ApiResponse<T>> {
        if (method === "event/subscribe") {
          subscribeParams = params as AppProtocol.SubscribeTimelineRequest;
          queueMicrotask(() => controller.abort());
          return { status: "success", data: { subscriptionId: "sub-session" } as T };
        }
        if (method === "event/unsubscribe") {
          return { status: "success", data: { unsubscribed: true } as T };
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
