import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";
import type { HttpAdapter, StreamAdapter } from "../adapters";
import type { AgentEvent, StartTurnRequest, TimelineEvent } from "../types";

describe("TalosClient streaming", () => {
  it("uses ack plus events for streaming messages", async () => {
    const http: HttpAdapter = {
      request: vi.fn().mockResolvedValue({
        requestId: "req-1",
        messageId: "msg-1",
        sessionId: "session-1",
        turnId: "turn-1",
        status: "accepted",
      }),
    };
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        sequence: 1,
        payload: {
          type: "turnCompleted",
          data: { stopReason: "endTurn" },
        },
        occurredAt: 1,
      })));
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");
    const startRequest: StartTurnRequest = {
      sessionId: "session-1",
      input: { content: [{ type: "text", text: "hello" }] },
      modelSelection: null,
      userPermissionMode: null,
      projectId: null,
      currentDirectory: null,
      clientRequestId: null,
    };

    await client.turns.startEvents(
      startRequest,
      () => undefined,
    );

    expect(http.request).toHaveBeenCalledWith(
      "http://localhost:8080/v1/turns/start",
      expect.objectContaining({
        method: "POST",
      }),
    );
    const [, startOptions] = vi.mocked(http.request).mock.calls[0];
    expect(JSON.parse(String(startOptions?.body))).toEqual({
      ...startRequest,
      clientRequestId: expect.any(String),
    });
    expect(stream).toHaveBeenCalledWith(
      "http://localhost:8080/v1/sessions/session-1/events",
      expect.objectContaining({
        method: "GET",
      }),
      expect.any(Function),
    );
  });

  it("filters timeline events by turn id in the SDK helper", async () => {
    const http: HttpAdapter = {
      request: vi.fn(),
    };
    const onEvent = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, options, onMessage) => {
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-other" },
        sequence: 1,
        payload: {
          type: "turnStarted",
          data: {},
        },
        occurredAt: 1,
      })));
      onMessage(JSON.stringify(timelineEvent({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        sequence: 2,
        payload: {
          type: "turnCompleted",
          data: { stopReason: "endTurn" },
        },
        occurredAt: 2,
      })));
      expect(options.signal.aborted).toBe(true);
    });
    const streamAdapter: StreamAdapter = { stream };
    const client = new TalosClient(http, streamAdapter, "http://localhost:8080");

    await client.turns.eventsForTurn("session-1", "turn-1", onEvent);

    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(onEvent).toHaveBeenCalledWith(expect.objectContaining({
      eventId: "ev_00000000000000000002",
      event: { type: "agent", data: expect.objectContaining({
        identity: { sessionId: "session-1", turnId: "turn-1" },
        payload: { type: "turnCompleted", data: { stopReason: "endTurn" } },
      }) },
    }));
  });

  it("keeps timeline streams persistent only when explicitly requested", async () => {
    const http: HttpAdapter = { request: vi.fn() };
    const stream = vi.fn().mockResolvedValue(undefined);
    const client = new TalosClient(http, { stream }, "http://localhost:8080");

    await client.turns.events("session-1", undefined, vi.fn(), undefined, { persistent: true });

    expect(stream.mock.calls[0]?.[0]).toBe(
      "http://localhost:8080/v1/sessions/session-1/events?close_when_idle=false",
    );
  });
});

function timelineEvent(event: AgentEvent): TimelineEvent {
  return {
    eventId: `ev_${String(event.sequence).padStart(20, "0")}`,
    eventSeq: event.sequence,
    persistedAt: event.occurredAt,
    event: { type: "agent", data: event },
  };
}
