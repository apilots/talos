import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient", () => {
  it("uses the desktop source type when starting desktop turns", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params?: unknown): Promise<T> {
        calls.push({ method, params });
        if (method === "turn.start") {
          return {
            request_id: "req-1",
            message_id: "message-1",
            session_id: "session-1",
            turn_id: "turn-1",
            status: "accepted",
          } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.turns.start("session-1", {
      input: [{ type: "text", text: "hello" }],
      context: {
        source_type: "http",
        provider: "sdk",
        user_id: "anonymous",
        channel_id: "web",
        message_id: "msg-1",
        trace_id: "trace-1",
      },
      model: "kimi-k2.5",
      project_id: undefined,
    });

    expect(calls).toEqual([
      {
        method: "turn.start",
        params: expect.objectContaining({
          session_id: "session-1",
          context: expect.objectContaining({
            source_type: "desktop",
            provider: "sdk",
            channel_id: "web",
            message_id: "msg-1",
            trace_id: "trace-1",
          }),
        }),
      },
    ]);
  });

  it("streams desktop turn events by message ack even when event request ids differ", async () => {
    let eventHandler: ((envelope: AppProtocol.TimelineEventEnvelope) => void) | undefined;
    const received: string[] = [];
    let subscribeParams: AppProtocol.EventSubscribeParams | undefined;
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params: unknown): Promise<T> {
        if (method === "event.subscribe") {
          subscribeParams = params as AppProtocol.EventSubscribeParams;
          queueMicrotask(() => {
            eventHandler?.(eventEnvelope("turn_started", "req-provider", "turn-1", "message-1"));
            eventHandler?.(eventEnvelope("turn_completed", "req-provider", "turn-1", "message-1"));
          });
          return { subscription_id: "sub-1" } as T;
        }
        if (method === "turn.start") {
          return {
            request_id: "req-client",
            message_id: "message-1",
            session_id: "session-1",
            turn_id: "turn-1",
            status: "accepted",
          } as T;
        }
        if (method === "event.unsubscribe") {
          return { unsubscribed: true } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: (handler) => {
        eventHandler = handler;
        return () => {
          eventHandler = undefined;
        };
      },
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.turns.startEvents(
      "session-1",
      { input: [{ type: "text", text: "hello" }] },
      (envelope) => received.push(envelope.event.event_type),
    );

    expect(received).toEqual(["turn_started", "turn_completed"]);
    expect(subscribeParams).toMatchObject({
      close_when_idle: true,
      close_on_terminal: true,
    });
  });
});

function eventEnvelope(
  eventType: "turn_started" | "turn_completed",
  requestId: string,
  turnId: string,
  messageId: string,
): AppProtocol.TimelineEventEnvelope {
  return {
    event_seq: 1n,
    event_id: `ev-${eventType}`,
    created_at_ms: 1n,
    turn_id: turnId,
    message_id: messageId,
    session_id: "session-1",
    event: {
      event_seq: 1n,
      event_id: `ev-${eventType}`,
      created_at_ms: 1n,
      turn_id: turnId,
      session_id: "session-1",
      message_id: messageId,
      sequence: 1n,
      event_type: eventType,
      payload:
        eventType === "turn_started"
          ? {
              type: "turn_started",
              request_id: requestId,
              turn_id: turnId,
              started_at_ms: 1n,
            }
          : {
              type: "turn_completed",
              request_id: requestId,
              turn_id: turnId,
              completed_at_ms: 2n,
              stop_reason: "end_turn",
              usage: usage(),
              metrics: null,
            },
    },
  };
}

function usage(): AppProtocol.TalosUsage {
  return {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  };
}
