import { afterEach, describe, expect, it, vi } from "vitest";
import type { AgentEvent, TimelineEventEnvelope } from "@talos/sdk-browser";

import {
  createInitialTurnRequest,
  retryTimelineEvents,
  sendTimelineEvents,
} from "./api";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

describe("chat api initial turn request", () => {
  it("builds first-turn metadata without session create configuration", () => {
    const request = createInitialTurnRequest("sess-1", "user-1", "hello", "qwen-max", undefined);

    expect(request).toMatchObject({
      session_id: "sess-1",
      input: [{ type: "text", text: "hello" }],
      model: "qwen-max",
      context: expect.objectContaining({
        user_id: "user-1",
        source_type: "http",
        provider: "sdk",
      }),
    });
    expect(request).not.toHaveProperty("conf");
  });

  it("keeps selected skills as structured input items", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "user-1",
      "use $docs",
      undefined,
      undefined,
      [
        {
          name: "docs",
          description: "Write docs",
          short_description: null,
          root: "/Users/test/.agents/skills/docs",
          path: "/Users/test/.agents/skills/docs/SKILL.md",
          scope: "external",
          source_plugin: null,
          source_install: null,
        },
      ],
    );

    expect(request.input).toEqual([
      { type: "text", text: "use $docs" },
      {
        type: "skill_selection",
        name: "docs",
        path: "/Users/test/.agents/skills/docs/SKILL.md",
      },
    ]);
  });
});

describe("chat message api", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("exposes the accepted server message id before streaming timeline events", async () => {
    const onAcceptedMessageId = vi.fn();
    const onEvent = vi.fn();
    const timelineEvent: AgentEvent = {
      event_seq: 1,
      event_id: "ev_00000000000000000001",
      created_at_ms: 1,
      event_type: "turn_completed",
      turn_id: "turn-1",
      message_id: "msg-accepted",
      session_id: "sess-1",
      sequence: 1,
      payload: {
        type: "turn_completed",
        request_id: "req-1",
        turn_id: "turn-1",
        completed_at_ms: 1,
        stop_reason: "end_turn",
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          total_tokens: 0,
          cache_creation_input_tokens: null,
          cache_read_input_tokens: null,
        },
        metrics: null,
      },
    };
    const event: TimelineEventEnvelope = {
      event_id: "ev_00000000000000000001",
      event_seq: 1,
      created_at_ms: 1,
      session_id: "sess-1",
      turn_id: "turn-1",
      message_id: "msg-accepted",
      event: timelineEvent,
    };
    const eventsForMessage = vi.fn(async (_sessionId, _messageId, callback) => {
      callback(event);
    });
    const start = vi.fn(async () => ({
      request_id: "req-1",
      message_id: "msg-accepted",
      session_id: "sess-1",
      turn_id: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      turns: {
        start,
        eventsForMessage,
      },
    } as unknown as TalosWebClient));

    await sendTimelineEvents(
      { session_id: "sess-1", input: [{ type: "text", text: "hello" }] },
      onEvent,
      undefined,
      onAcceptedMessageId,
    );

    expect(onAcceptedMessageId).toHaveBeenCalledWith("msg-accepted");
    expect(eventsForMessage).toHaveBeenCalledWith(
      "sess-1",
      "msg-accepted",
      expect.any(Function),
      undefined,
    );
    expect(onEvent).toHaveBeenCalledWith(event);
  });

  it("exposes the accepted retry message id before streaming retry events", async () => {
    const onAcceptedMessageId = vi.fn();
    const retry = vi.fn(async () => ({
      request_id: "req-1",
      message_id: "msg-retry",
      session_id: "sess-1",
      turn_id: "turn-1",
      status: "accepted",
    }));
    const eventsForMessage = vi.fn(async () => undefined);
    setTalosClientFactory(() => ({
      turns: {
        retry,
        eventsForMessage,
      },
    } as unknown as TalosWebClient));

    await retryTimelineEvents(
      {
        session_id: "sess-1",
        client_message_id: "client-1",
        retry_of_message_id: "msg-old",
        input: [{ type: "text", text: "hello" }],
      },
      vi.fn(),
      undefined,
      onAcceptedMessageId,
    );

    expect(onAcceptedMessageId).toHaveBeenCalledWith("msg-retry");
    expect(eventsForMessage).toHaveBeenCalledWith(
      "sess-1",
      "msg-retry",
      expect.any(Function),
      undefined,
    );
  });
});
