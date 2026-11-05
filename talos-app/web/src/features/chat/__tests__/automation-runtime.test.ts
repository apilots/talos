import { beforeEach, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-browser";
import { automationTurnTiming } from "../automation-runtime";
import { applyActiveTurnSnapshot, applyStreamEvent, clearInactiveTurnSnapshot, restoreActiveTurn } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import { activeTurn, historyTextItem, itemStarted, sessionHistory, turnCompleted, turnMetrics, userInputStarted, withMessage } from "./streaming-fixtures";

const origin = { type: "loop" as const, loopId: "loop-1", occurrenceId: "occ-1", configRevision: 3 };
beforeEach(() => useAppStore.setState({ streams: {} }));
it("uses the canonical triggering message time and ignores a later human Steer", () => {
  applyActiveTurnSnapshot("sess-1", activeTurn({ automationInput: { messageId: "msg-first", origin } }));
  const first = userInputStarted("first", "same", 1);
  if (first.event.data.payload.type !== "messageStarted") throw new Error("Invalid fixture");
  first.event.data.payload.data.message.createdAt = 900;
  applyStreamEvent("sess-1", first);
  applyStreamEvent("sess-1", userInputStarted("steer", "same", 2));
  expect(automationTurnTiming("sess-1", "turn-1", useAppStore.getState().streams["sess-1"], undefined)).toEqual({ turnId: "turn-1", origin, startedAt: 900 });
  clearInactiveTurnSnapshot("sess-1");
  expect(automationTurnTiming("sess-1", undefined, useAppStore.getState().streams["sess-1"], undefined)).toBeUndefined();
});
it("recovers the same canonical start from a loaded snapshot without another query", () => {
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "check")]);
  history.data[0]!.result.result.messages[0]!.createdAt = 700;
  const snapshot = activeTurn({ automationInput: { messageId: "msg-first", origin }, messages: history.data[0]!.result.result.messages });
  expect(automationTurnTiming("sess-1", "turn-1", undefined, snapshot)).toEqual({ turnId: "turn-1", origin, startedAt: 700 });
  restoreActiveTurn("sess-1", snapshot);
  expect(automationTurnTiming("sess-1", "turn-1", useAppStore.getState().streams["sess-1"], undefined)).toEqual({ turnId: "turn-1", origin, startedAt: 700 });
  expect(useAppStore.getState().streams["sess-1"]!.turnMetrics?.modelRounds).toBe(1);
});
it("does not use a previous Turn or invent a start when the exact message has not arrived", () => {
  const snapshot = activeTurn({ automationInput: { messageId: "msg-first", origin } });
  expect(automationTurnTiming("sess-1", "turn-2", undefined, snapshot)).toBeUndefined();
  expect(automationTurnTiming("sess-1", "turn-1", undefined, snapshot)).toBeUndefined();
  expect(automationTurnTiming("sess-1", undefined, undefined, snapshot)).toBeUndefined();
});
it("retains current metrics across same-Turn provenance updates and human Steer", () => {
  const snapshot = activeTurn({ lastEventSeq: 1, automationInput: { messageId: "msg-first", origin } });
  restoreActiveTurn("sess-1", snapshot);
  applyStreamEvent("sess-1", turnMetrics(2, { total_tokens: 42 }));
  applyActiveTurnSnapshot("sess-1", snapshot);
  applyStreamEvent("sess-1", userInputStarted("steer", "continue", 3));
  expect(useAppStore.getState().streams["sess-1"]!.turnMetrics).toMatchObject({ state: "running", usage: { total_tokens: 42 } });
});

it.each(["completed", "failed"])("rejects an old running snapshot after the same Turn is %s, including after active cleanup", (outcome) => {
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "check")]);
  const snapshot = activeTurn({ lastEventSeq: 1, automationInput: { messageId: "msg-first", origin }, messages: history.data[0]!.result.result.messages });
  restoreActiveTurn("sess-1", snapshot);
  const terminal = turnCompleted(2);
  if (outcome === "failed") terminal.event.data.payload = { type: "turnFailed", data: { error: { code: AppProtocol.ErrorCode.InternalError, message: "Execution failed", localization: null }, retryable: false } };
  applyStreamEvent("sess-1", terminal);
  expect(automationTurnTiming("sess-1", "turn-1", useAppStore.getState().streams["sess-1"], snapshot)).toBeUndefined();
  clearInactiveTurnSnapshot("sess-1");
  expect(automationTurnTiming("sess-1", "turn-1", useAppStore.getState().streams["sess-1"], snapshot)).toBeUndefined();
  const next = activeTurn({ identity: { sessionId: "sess-1", turnId: "turn-2" }, automationInput: { messageId: "msg-next", origin: { ...origin, occurrenceId: "occ-next" } }, messages: [{ ...snapshot.messages[0]!, turnId: "turn-2", messageId: "msg-next", createdAt: 1000 }] });
  expect(automationTurnTiming("sess-1", "turn-2", useAppStore.getState().streams["sess-1"], next)).toEqual({ turnId: "turn-2", origin: next.automationInput!.origin, startedAt: 1000 });
  applyActiveTurnSnapshot("sess-1", next);
  expect(automationTurnTiming("sess-1", "turn-2", useAppStore.getState().streams["sess-1"], next)).toEqual({ turnId: "turn-2", origin: next.automationInput!.origin, startedAt: 1000 });
  const input = withMessage(userInputStarted("next", "next run", 3, [], "msg-next"), "msg-next", "turn-2");
  if (input.event.data.payload.type !== "messageStarted") throw new Error("Invalid fixture");
  input.event.data.payload.data.message.createdAt = 1000;
  applyStreamEvent("sess-1", input);
  applyStreamEvent("sess-1", withMessage(itemStarted("next-thinking", "assistant_text", "commentary", 4), "msg-agent-next", "turn-2"));
  const stream = useAppStore.getState().streams["sess-1"]!;
  expect(automationTurnTiming("sess-1", "turn-2", stream, next)).toEqual({ turnId: "turn-2", origin: next.automationInput!.origin, startedAt: 1000 });
  expect(stream.blocks.find((block) => block.kind === "timeline" && block.id === "timeline-sess-1-turn-2-msg-next")).not.toHaveProperty("metrics", expect.anything());
  expect(stream.blocks.find((block) => block.kind === "timeline" && block.id === "timeline-sess-1-turn-1-msg-first")).toHaveProperty("metrics.state", outcome);
});
