import { describe, expect, it } from "vitest";
import type { AppProtocol } from "@talos/sdk-core";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { createDesktopTalosClient } from "./desktop-client";

describe("createDesktopTalosClient", () => {
  it("uses the desktop source type when creating desktop sessions", async () => {
    const calls: Array<{ method: AppProtocol.ClientMethod; params: unknown }> = [];
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod, params?: unknown): Promise<T> {
        calls.push({ method, params });
        if (method === "session.create") {
          return {
            context: { session_id: "session-1" },
          } as T;
        }
        throw new Error(`unexpected method: ${method}`);
      },
      onEvent: () => () => undefined,
      onSubscriptionTerminated: () => () => undefined,
    };
    const client = createDesktopTalosClient(bridge);

    await client.sessions.create({
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
        method: "session.create",
        params: expect.objectContaining({
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
    let eventHandler: ((envelope: AppProtocol.ApplicationEventEnvelope) => void) | undefined;
    const received: string[] = [];
    const bridge: DesktopRendererBridge = {
      invoke: async function <T>(method: AppProtocol.ClientMethod): Promise<T> {
        if (method === "event.subscribe") {
          queueMicrotask(() => {
            eventHandler?.(eventEnvelope("message_started", "req-provider", "turn-1", "message-1"));
            eventHandler?.(eventEnvelope("message_completed", "req-provider", "turn-1", "message-1"));
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

    await client.messages.sendEvents(
      "session-1",
      { input: [{ type: "text", text: "hello" }] },
      (event) => received.push(event.event_type),
    );

    expect(received).toEqual(["message_started", "message_completed"]);
  });
});

function eventEnvelope(
  eventType: "message_started" | "message_completed",
  requestId: string,
  turnId: string,
  messageId: string,
): AppProtocol.ApplicationEventEnvelope {
  return {
    event_id: `ev-${eventType}`,
    subscription_id: "sub-1",
    turn_id: turnId,
    event: {
      session_id: "session-1",
      message_id: messageId,
      sequence: 1n,
      event_type: eventType,
      payload:
        eventType === "message_started"
          ? {
              type: "message_started",
              request_id: requestId,
              model: "model-a",
              usage: usage(),
              iteration: 1n,
              step_id: "step-1",
            }
          : {
              type: "message_completed",
              request_id: requestId,
              stop_reason: "end_turn",
              usage: usage(),
              total_output_items: 1n,
              iteration: 1n,
              step_id: "step-1",
              task_count: 0,
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
