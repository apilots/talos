import { beforeEach, expect, it } from "vitest";
import { buildBlocksFromHistorySnapshot, applyActiveTurnSnapshot, applyStreamEvent, clearInactiveTurnSnapshot, restoreActiveTurn } from "../stream-events";
import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { activeTurn, historyTextItem, sessionHistory, turnCompleted, userInputStarted } from "./streaming-fixtures";
import { useAppStore } from "@/shared/store/app-store";
const origin = { type: "goal" as const, goalId: "goal-1", objectiveRevision: 2 };
beforeEach(() => useAppStore.setState({ streams: {} }));
it("labels only the exact triggering Input, even when a later human input has identical text", () => {
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "same"), historyTextItem("steer", "user_input", "input", "same")]);
  history.data[0]!.automationInput = { messageId: "msg-first", origin };
  const blocks = buildBlocksFromHistorySnapshot(history).blocks.filter((block) => block.kind === "user");
  expect(blocks[0]).toMatchObject({ automationOrigin: origin });
  expect(blocks[1]).not.toHaveProperty("automationOrigin");
});
it("updates late provenance at the same canonical offset without replaying or tagging subsequent Steer", () => {
  applyStreamEvent("sess-1", userInputStarted("first", "same", 1));
  applyStreamEvent("sess-1", userInputStarted("steer", "same", 2));
  const before = useAppStore.getState().streams["sess-1"]!;
  const snapshot = activeTurn({ lastEventSeq: before.lastEventSeq, automationInput: { messageId: "msg-first", origin } });
  restoreActiveTurn("sess-1", snapshot);
  // An older unbound read cannot erase a binding learned later in this Turn.
  applyActiveTurnSnapshot("sess-1", { ...snapshot, automationInput: null });
  const streamState = useAppStore.getState().streams["sess-1"]!;
  expect(streamState.lastEventSeq).toBe(before.lastEventSeq);
  const blocks = buildSessionBlocks({ sessionId: "sess-1", streamState,
    history: buildBlocksFromHistorySnapshot(undefined), waitingResponseDetail: "", waitingResponseTitle: "", stoppingResponseDetail: "", stoppingResponseTitle: "" }).filter((block) => block.kind === "user");
  expect(blocks).toHaveLength(2);
  expect(blocks[0]).toMatchObject({ automationOrigin: origin });
  expect(blocks[1]).not.toHaveProperty("automationOrigin");
});
it.each([
  origin,
  { type: "loop" as const, loopId: "loop-1", occurrenceId: "occ-1", configRevision: 3 },
])("retains admitted $type input provenance after Steer and active-Turn cleanup", (inputOrigin) => {
  applyStreamEvent("sess-1", userInputStarted("first", "same", 1));
  const snapshot = activeTurn({ lastEventSeq: 1, automationInput: { messageId: "msg-first", origin: inputOrigin } });
  restoreActiveTurn("sess-1", snapshot);
  applyStreamEvent("sess-1", userInputStarted("steer", "same", 2));
  applyStreamEvent("sess-1", turnCompleted(3));
  clearInactiveTurnSnapshot("sess-1");
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "same"), historyTextItem("steer", "user_input", "input", "same")]);
  history.data[0]!.automationInput = { messageId: "msg-first", origin: inputOrigin };
  // Inactive metadata can arrive before the committed history query settles.
  for (const loadedHistory of [undefined, history]) {
    const blocks = buildSessionBlocks({ sessionId: "sess-1", streamState: useAppStore.getState().streams["sess-1"],
      history: buildBlocksFromHistorySnapshot(loadedHistory), waitingResponseDetail: "", waitingResponseTitle: "", stoppingResponseDetail: "", stoppingResponseTitle: "" }).filter((block) => block.kind === "user");
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toMatchObject({ automationOrigin: inputOrigin });
    expect(blocks[1]).not.toHaveProperty("automationOrigin");
  }
});
it.each([
  origin,
  { type: "loop" as const, loopId: "loop-1", occurrenceId: "occ-1", configRevision: 3 },
])("binds $type provenance when the active snapshot arrives before its triggering message", (inputOrigin) => {
  applyActiveTurnSnapshot("sess-1", activeTurn({ automationInput: { messageId: "msg-first", origin: inputOrigin } }));
  applyStreamEvent("sess-1", userInputStarted("first", "same", 1));
  applyStreamEvent("sess-1", userInputStarted("steer", "same", 2));
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "same"), historyTextItem("steer", "user_input", "input", "same")]);
  // A newer snapshot without the late binding must not erase already known provenance.
  restoreActiveTurn("sess-1", activeTurn({ lastEventSeq: 3, messages: history.data[0]!.result.result.messages }));
  clearInactiveTurnSnapshot("sess-1");
  const blocks = useAppStore.getState().streams["sess-1"]!.blocks.filter((block) => block.kind === "user");
  expect(blocks[0]).toMatchObject({ automationOrigin: inputOrigin });
  expect(blocks[1]).not.toHaveProperty("automationOrigin");
});
it.each([
  origin,
  { type: "loop" as const, loopId: "loop-1", occurrenceId: "occ-1", configRevision: 3 },
])("fills late committed $type provenance without changing live input content", (inputOrigin) => {
  applyStreamEvent("sess-1", userInputStarted("first", "live input", 1, ["live-file"]));
  applyStreamEvent("sess-1", userInputStarted("steer", "live input", 2));
  applyStreamEvent("sess-1", turnCompleted(3));
  clearInactiveTurnSnapshot("sess-1");
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "history input"), historyTextItem("steer", "user_input", "input", "live input")]);
  history.data[0]!.automationInput = { messageId: "msg-first", origin: inputOrigin };
  const blocks = buildSessionBlocks({ sessionId: "sess-1", streamState: useAppStore.getState().streams["sess-1"],
    history: buildBlocksFromHistorySnapshot(history), waitingResponseDetail: "", waitingResponseTitle: "", stoppingResponseDetail: "", stoppingResponseTitle: "" }).filter((block) => block.kind === "user");
  expect(blocks).toHaveLength(2);
  expect(blocks[0]).toMatchObject({ automationOrigin: inputOrigin, text: "live input", fileIds: ["live-file"] });
  expect(blocks[1]).not.toHaveProperty("automationOrigin");
});
