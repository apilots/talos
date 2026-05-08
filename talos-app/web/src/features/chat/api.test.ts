import { afterEach, describe, expect, it, vi } from "vitest";
import type { TalosEvent } from "@talos/sdk-browser";

import {
  createSession,
  retryMessageEvents,
  sendMessageEvents,
  updateSessionApprovalMode,
} from "./api";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

describe("chat api approval mode wiring", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("sends session approval mode in create request", async () => {
    const create = vi.fn(async () => ({
      context: { session_id: "session-1" },
      conf: {
        tools: {
          approval_policy: "always",
        },
      },
    }));
    setTalosClientFactory(() => ({
      sessions: {
        create,
      },
    } as unknown as TalosWebClient));

    const session = await createSession("user-1", "qwen-max", undefined, "needs_approval");

    expect(create).toHaveBeenCalledWith(
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
    const patch = vi.fn(async () => undefined);
    setTalosClientFactory(() => ({
      sessions: {
        patch,
      },
    } as unknown as TalosWebClient));

    await updateSessionApprovalMode("session-2", "skip");

    expect(patch).toHaveBeenCalledWith("session-2", {
      conf: {
        tools: {
          approval_policy: "never",
        },
      },
    });
  });
});

describe("chat message api", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("exposes the accepted server message id before streaming message events", async () => {
    const onAcceptedMessageId = vi.fn();
    const onEvent = vi.fn();
    const event: TalosEvent = {
      event_type: "message_completed",
      message_id: "msg-accepted",
      session_id: "sess-1",
      sequence: 1,
      payload: {
        type: "message_completed",
        request_id: "req-1",
        stop_reason: "end_turn",
        usage: {
          input_tokens: 0,
          output_tokens: 0,
          total_tokens: 0,
          cache_creation_input_tokens: null,
          cache_read_input_tokens: null,
        },
        total_output_items: 1n,
        iteration: 0n,
        step_id: "step-1",
        task_count: 0,
      },
    };
    const eventsForMessage = vi.fn(async (_sessionId, _messageId, callback) => {
      callback(event);
    });
    const send = vi.fn(async () => ({
      request_id: "req-1",
      message_id: "msg-accepted",
      session_id: "sess-1",
      turn_id: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      messages: {
        send,
        eventsForMessage,
      },
    } as unknown as TalosWebClient));

    await sendMessageEvents("sess-1", "hello", onEvent, undefined, onAcceptedMessageId);

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
      messages: {
        retry,
        eventsForMessage,
      },
    } as unknown as TalosWebClient));

    await retryMessageEvents(
      "sess-1",
      {
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
