import { beforeEach, expect, it } from "vitest";
import { applyStreamEvent, buildBlocksFromHistorySnapshot, restoreActiveTurn } from "../stream-events";
import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { activeTurn, historyTextItem, sessionHistory, userInputStarted } from "./streaming-fixtures";
import { useAppStore } from "@/shared/store/app-store";
import type { AgentMessage, AutomationMode } from "@talos/sdk-browser";

function selectMode(message: AgentMessage, mode: AutomationMode) {
  message.blocks.push({ ...message.blocks[0]!, blockId: `selected-${mode}`, content: { type: "userContent", data: { type: "skillSelection", name: mode, path: `/actual-worker/${mode}/SKILL.md` } } });
}
function visibleBlocks(history = buildBlocksFromHistorySnapshot(undefined)) {
  return buildSessionBlocks({ sessionId: "sess-1", streamState: useAppStore.getState().streams["sess-1"], history,
    waitingResponseDetail: "", waitingResponseTitle: "", stoppingResponseDetail: "", stoppingResponseTitle: "" }).filter((block) => block.kind === "user");
}
beforeEach(() => useAppStore.setState({ streams: {} }));

it.each(["goal", "loop"] as const)("shows the original %s selection before binding and after canonical/history replacement", (mode) => {
  useAppStore.getState().setPendingInput("sess-1", { text: "same text", fileIds: [], activationMode: mode, clientMessageId: "local", phase: "local" });
  expect(visibleBlocks()).toMatchObject([{ text: "same text", activationMode: mode }]);
  useAppStore.getState().setPendingInput("sess-1", { text: "same text", fileIds: [], activationMode: mode, clientMessageId: "local", serverMessageId: "msg-first", phase: "accepted" });
  const first = userInputStarted("first", "same text", 1);
  if (first.event.type !== "agent" || first.event.data.payload.type !== "messageStarted") throw new Error("Expected User input fixture");
  selectMode(first.event.data.payload.data.message, mode);
  applyStreamEvent("sess-1", first);
  applyStreamEvent("sess-1", userInputStarted("steer", "same text", 2));
  const live = visibleBlocks();
  expect(live).toHaveLength(2);
  expect(live[0]).toMatchObject({ text: "same text", activationMode: mode });
  expect(live[0]).not.toHaveProperty("automationOrigin");
  expect(live[1]).not.toHaveProperty("activationMode");
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "same text"), historyTextItem("steer", "user_input", "input", "same text")]);
  selectMode(history.data[0]!.result.result.messages[0]!, mode);
  const restored = buildBlocksFromHistorySnapshot(history).blocks.filter((block) => block.kind === "user");
  expect(restored[0]).toMatchObject({ activationMode: mode });
  expect(restored[0]).not.toHaveProperty("automationOrigin");
  expect(restored[1]).not.toHaveProperty("activationMode");
});

it("fills only same-ID missing activation metadata while keeping original live content and real origin", () => {
  applyStreamEvent("sess-1", userInputStarted("first", "live text", 1, ["live-file"]));
  applyStreamEvent("sess-1", userInputStarted("steer", "live text", 2));
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "older text"), historyTextItem("steer", "user_input", "input", "live text")]);
  selectMode(history.data[0]!.result.result.messages[0]!, "goal");
  const bound = { type: "goal" as const, goalId: "actual-goal", objectiveRevision: 1 };
  history.data[0]!.automationInput = { messageId: "msg-first", origin: bound };
  const blocks = visibleBlocks(buildBlocksFromHistorySnapshot(history));
  expect(blocks).toHaveLength(2);
  expect(blocks[0]).toMatchObject({ text: "live text", fileIds: ["live-file"], activationMode: "goal", automationOrigin: bound });
  expect(blocks[1]).not.toHaveProperty("activationMode");
});

it("retains a learned selection on same-ID snapshot restore without copying it to a different input", () => {
  const first = userInputStarted("first", "live text", 1);
  if (first.event.type !== "agent" || first.event.data.payload.type !== "messageStarted") throw new Error("Expected User input fixture");
  selectMode(first.event.data.payload.data.message, "loop");
  applyStreamEvent("sess-1", first);
  const history = sessionHistory([historyTextItem("first", "user_input", "input", "canonical text"), historyTextItem("steer", "user_input", "input", "live text")]);
  restoreActiveTurn("sess-1", activeTurn({ lastEventSeq: 3, messages: history.data[0]!.result.result.messages }));
  const blocks = visibleBlocks();
  expect(blocks[0]).toMatchObject({ text: "canonical text", activationMode: "loop" });
  expect(blocks[1]).not.toHaveProperty("activationMode");
});


it("fills only the accepted pending message's canonical block without duplicate cards or Steer labels", () => {
  useAppStore.getState().setPendingInput("sess-1", { text: "original draft", fileIds: [], activationMode: "goal", clientMessageId: "local", phase: "local" });
  applyStreamEvent("sess-1", userInputStarted("first", "canonical text", 1));
  useAppStore.getState().setPendingInput("sess-1", { text: "original draft", fileIds: [], activationMode: "goal", clientMessageId: "local", serverMessageId: "msg-first", phase: "accepted" });
  applyStreamEvent("sess-1", userInputStarted("steer", "canonical text", 2));
  const blocks = visibleBlocks();
  expect(blocks).toHaveLength(2);
  expect(blocks[0]).toMatchObject({ text: "canonical text", activationMode: "goal" });
  expect(blocks[0]).not.toHaveProperty("automationOrigin");
  expect(blocks[1]).not.toHaveProperty("activationMode");
});
