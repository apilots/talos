import { describe, expect, it } from "vitest";

import type { AgentEvent } from "@talos/sdk-core";

import { LiveTurnProjection } from "../live-turn-projection";

describe("LiveTurnProjection", () => {
  it("replaces streamed block state with the canonical completed message", () => {
    const accumulator = new LiveTurnProjection();
    const event = (sequence: number, payload: AgentEvent["payload"]): AgentEvent => ({
      identity: { sessionId: "session-1", turnId: "turn-1" },
      sequence,
      occurredAt: sequence,
      payload,
    });
    const startedMessage = {
      messageId: "message-1",
      turnId: "turn-1",
      role: "agent" as const,
      status: "inProgress" as const,
      blocks: [],
      createdAt: 1,
      completedAt: null,
    };
    const startedBlock = {
      blockId: "block-1",
      status: "running" as const,
      content: { type: "assistantText" as const, data: { phase: "pending" as const, text: "" } },
      startedAt: 2,
      completedAt: null,
    };

    accumulator.apply(event(1, { type: "messageStarted", data: { message: startedMessage } }));
    accumulator.apply(
      event(2, {
        type: "messageBlockStarted",
        data: { messageId: "message-1", block: startedBlock },
      }),
    );
    accumulator.apply(
      event(3, {
        type: "messageBlockDelta",
        data: {
          messageId: "message-1",
          blockId: "block-1",
          delta: { type: "text", data: "provisional" },
        },
      }),
    );
    accumulator.apply(
      event(4, {
        type: "messageCompleted",
        data: {
          ...startedMessage,
          status: "completed",
          completedAt: 4,
          blocks: [
            {
              ...startedBlock,
              status: "completed",
              completedAt: 4,
              content: {
                type: "assistantText",
                data: { phase: "finalAnswer", text: "canonical" },
              },
            },
          ],
        },
      }),
    );

    expect(accumulator.block("message-1", "block-1")).toEqual({
      ...startedBlock,
      status: "completed",
      completedAt: 4,
      content: {
        type: "assistantText",
        data: { phase: "finalAnswer", text: "canonical" },
      },
    });
  });
});
