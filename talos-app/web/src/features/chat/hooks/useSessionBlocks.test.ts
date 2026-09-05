import { beforeEach, describe, expect, it } from "vitest";

import {
  historyTextItem,
  itemStarted,
  sessionHistory,
  textDelta,
  userInputStarted,
} from "../__tests__/streaming-fixtures";
import { applyStreamEvent } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import { buildSessionBlocks } from "./useSessionBlocks";

describe("buildSessionBlocks", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("renders one user block when history and the live stream own the same message", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "single input",
      fileIds: [],
      clientMessageId: "client-1",
      serverMessageId: "msg-input-1",
      phase: "accepted",
    });
    applyStreamEvent("sess-1", userInputStarted("input-1", "single input", 1));
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 2));
    applyStreamEvent("sess-1", textDelta("thinking-1", "Working.", 3));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      sessionHistory: sessionHistory([
        historyTextItem("input-1", "user_input", "input", "single input"),
      ]),
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.filter((block) => block.kind === "user")).toEqual([
      expect.objectContaining({ id: "user-turn-1-msg-input-1", text: "single input" }),
    ]);
  });

  it("reconciles interleaved same-turn inputs and model work by timeline item", () => {
    applyStreamEvent("sess-1", userInputStarted("input-1", "initial", 1));
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 2));
    applyStreamEvent("sess-1", textDelta("thinking-1", "before steer", 3));
    applyStreamEvent("sess-1", userInputStarted("input-2", "steer", 4));
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "final_answer", 5));
    applyStreamEvent("sess-1", textDelta("answer-1", "after steer", 6));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      sessionHistory: sessionHistory([
        historyTextItem("input-1", "user_input", "input", "initial"),
        historyTextItem("thinking-1", "assistant_text", "commentary", "before steer"),
        historyTextItem("input-2", "user_input", "input", "steer"),
        historyTextItem("answer-1", "assistant_text", "final_answer", "after steer"),
      ]),
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks).toEqual([
      expect.objectContaining({ id: "user-turn-1-msg-input-1", text: "initial" }),
      expect.objectContaining({
        id: "timeline-sess-1-turn-1-msg-input-1",
        items: [expect.objectContaining({ text: "before steer" })],
      }),
      expect.objectContaining({ id: "user-turn-1-msg-input-2", text: "steer" }),
      expect.objectContaining({
        id: "timeline-sess-1-turn-1-msg-input-2",
        items: [expect.objectContaining({ text: "after steer" })],
      }),
    ]);
  });

  it("renders an attachment-only pending input", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "",
      fileIds: ["file-1"],
      clientMessageId: "client-1",
      phase: "local",
    });

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks).toContainEqual(
      expect.objectContaining({
        kind: "user",
        text: "",
        fileIds: ["file-1"],
      }),
    );
  });
});
