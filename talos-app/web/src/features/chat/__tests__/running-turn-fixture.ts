import type { ActiveTurnSnapshot } from "@talos/sdk-core";
import { activeTurn, executionMetrics } from "./streaming-fixtures";

export function runningTurn(overrides: Partial<ActiveTurnSnapshot> = {}): ActiveTurnSnapshot {
  return activeTurn({
    lastEventId: "ev_9", lastEventSeq: 9, lastSequence: 9,
    metrics: executionMetrics(42),
    messages: [{
      messageId: "msg-1", turnId: "turn-1", role: "agent", status: "inProgress",
      createdAt: 1, completedAt: null,
      blocks: [{
        blockId: "answer", status: "running", startedAt: 1, completedAt: null,
        content: { type: "assistantText", data: { phase: "finalAnswer", text: "base" } },
      }],
    }],
    ...overrides,
  });
}
