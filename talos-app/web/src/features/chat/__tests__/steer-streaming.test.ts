import { beforeEach, describe, expect, it } from "vitest";

import { applyStreamEvent } from "../stream-events";
import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { useAppStore } from "@/shared/store/app-store";
import { itemCompleted, itemStarted, textDelta, userInputStarted, withMessage } from "./streaming-fixtures";

describe("same-turn user input streaming", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("keeps accepted input visible until its matching canonical message arrives", () => {
    const state = useAppStore.getState();
    state.setPendingInput("sess-1", {
      text: "Current question", fileIds: [], clientMessageId: "client-current",
      serverMessageId: "msg-current", phase: "accepted",
    });
    state.setStreamMeta("sess-1", { lastEventSeq: 10, isStreaming: true });
    applyStreamEvent("sess-1", userInputStarted("old", "Old question", 2));
    const started = userInputStarted("current", "Current question", 11, [], "msg-current");
    if (started.event.payload.type !== "messageStarted") throw new Error("Invalid fixture");
    const completedMessage = structuredClone(started.event.payload.data.message);
    started.event.payload.data.message.blocks = [];
    applyStreamEvent("sess-1", started);
    expect(useAppStore.getState().streams["sess-1"]?.pendingInput).toMatchObject({
      text: "Current question", serverMessageId: "msg-current", phase: "accepted",
    });
    applyStreamEvent("sess-1", {
      ...started, eventSeq: 12, eventId: "ev_12",
      event: { ...started.event, sequence: 12, payload: { type: "messageCompleted", data: completedMessage } },
    });
    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual([
      { id: "user-turn-1-msg-current", kind: "user", text: "Current question", fileIds: [], live: false },
    ]);
  });

  it("renders server user input without duplicating completed pending input", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "hello from local",
      fileIds: [],
      clientMessageId: "client-1",
      serverMessageId: "msg-input-1",
      phase: "accepted",
    });

    applyStreamEvent("sess-1", userInputStarted("input-1", "hello from server", 1));
    applyStreamEvent("sess-1", withMessage(itemCompleted("input-1", 2, "user_input"), "msg-input-1", "turn-1"));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.filter((block) => block.kind === "user")).toEqual([
      expect.objectContaining({ id: "user-turn-1-msg-input-1", text: "hello from server" }),
    ]);
    expect(useAppStore.getState().streams["sess-1"]?.pendingInput?.phase).toBe("completed");
  });

  it("uses the backend-acknowledged message id for a pending user block", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "hello from local",
      fileIds: [],
      clientMessageId: "client-1",
      serverMessageId: "msg-1",
      phase: "accepted",
    });

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks).toEqual([
      expect.objectContaining({ id: "user-msg-1", kind: "user", live: true }),
    ]);
  });

  it("interleaves user inputs with model work", () => {
    applyStreamEvent("sess-1", userInputStarted("input-1", "initial", 1));
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 2));
    applyStreamEvent("sess-1", textDelta("thinking-1", "before steer", 3));
    applyStreamEvent("sess-1", userInputStarted("input-2", "steer", 4, ["file-1"]));
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "final_answer", 5));
    applyStreamEvent("sess-1", textDelta("answer-1", "after steer", 6));

    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual([
      expect.objectContaining({ id: "user-turn-1-msg-input-1", text: "initial", fileIds: [] }),
      expect.objectContaining({
        id: "timeline-sess-1-turn-1-msg-input-1",
        items: [expect.objectContaining({ text: "before steer" })],
      }),
      expect.objectContaining({ id: "user-turn-1-msg-input-2", text: "steer", fileIds: ["file-1"] }),
      expect.objectContaining({
        id: "timeline-sess-1-turn-1-msg-input-2",
        items: [expect.objectContaining({ text: "after steer" })],
      }),
    ]);
  });
});
