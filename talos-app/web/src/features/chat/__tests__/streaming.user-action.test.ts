import { beforeEach, describe, expect, it } from "vitest";

import {
  applyActiveTurnSnapshot,
  applyStreamEvent,
  applyTimelineRecovery,
} from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import {
  activeTurn,
  itemStarted,
  pendingUserAction,
  textDelta,
  toolResult,
  turnCompleted,
  userInputStarted,
  waitingForUserAction,
  withMessage,
} from "./streaming-fixtures";

describe("timeline user action events", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("keeps the turn live while the canonical event waits for user action", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
    applyStreamEvent("sess-1", waitingForUserAction(2));

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream).toMatchObject({
      activeTurnId: "turn-1",
      isStreaming: true,
      connectionState: "detached",
      lastEventId: "ev_2",
      pendingUserAction: { actionId: "action-1", itemId: "tool-1" },
    });
    expect(stream?.blocks).toEqual([
      expect.objectContaining({
        kind: "timeline",
        live: true,
        items: [expect.objectContaining({ itemId: "tool-1", status: "waitingForInput" })],
      }),
    ]);

    applyStreamEvent("sess-1", turnCompleted(3));
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      isStreaming: false,
      connectionState: "idle",
      pendingUserAction: undefined,
    });
  });

  it("does not advance the consumed cursor from an active turn index", () => {
    useAppStore.getState().setStreamMeta("sess-1", { lastEventId: "ev_7" });
    applyActiveTurnSnapshot("sess-1", activeTurn({
      state: "running",
      initialMessageId: "msg-1",
      lastEventId: "ev_9",
    }));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeTurnId: "turn-1",
      activeMessageId: "msg-1",
      lastEventId: "ev_7",
      connectionState: "detached",
      isStreaming: true,
    });
  });

  it("restores a cancelling turn without treating it as terminal", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({ state: "cancelling" }));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeTurnId: "turn-1",
      isStreaming: true,
      connectionState: "cancelling",
    });
  });

  it("does not rewind a newer consumed cursor to the recovery high-water mark", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      activeTurnId: "turn-1",
      activeMessageId: "msg-1",
      isStreaming: true,
      lastEventId: "ev_12",
      lastEventSeq: 12,
    });

    applyTimelineRecovery("sess-1", {
      replay: {
        events: [],
        highWaterMarkEventId: "ev_9",
        nextCursor: null,
        lastEventSeq: 9,
        cursorStatus: "fromBeginning",
      },
      activeTurn: activeTurn({ lastEventId: "ev_9" }),
    });

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_12",
      lastEventSeq: 12,
    });
  });

  it("does not advance the consumed cursor to an unseen recovery high-water mark", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      lastEventId: "ev_1",
      lastEventSeq: 1,
    });

    applyTimelineRecovery("sess-1", {
      replay: {
        events: [],
        highWaterMarkEventId: "ev_9",
        nextCursor: "ev_1",
        lastEventSeq: 1,
        cursorStatus: "current",
      },
      activeTurn: activeTurn({ lastEventId: "ev_9" }),
    });

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_1",
      lastEventSeq: 1,
    });
  });

  it("uses the canonical turn identity independently of message identity", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({}));
    applyStreamEvent(
      "sess-1",
      withMessage(itemStarted("tool-1", "tool_call", "acting", 10), "msg-new", "turn-new"),
    );

    expect(useAppStore.getState().streams["sess-1"]?.activeTurnId).toBe("turn-new");
  });

  it("restores pending action content from replay, not from the active turn index", () => {
    applyTimelineRecovery("sess-1", {
      replay: {
        events: [waitingForUserAction(2)],
        highWaterMarkEventId: "ev_2",
        nextCursor: null,
        lastEventSeq: 2,
        cursorStatus: "fromBeginning",
      },
      activeTurn: activeTurn({ state: "waitingForInput", lastEventId: "ev_2" }),
    });

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream?.pendingUserAction).toMatchObject({ actionId: "action-1" });
    const timeline = stream?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({ itemId: "tool-1", status: "waitingForInput" })],
    });
  });

  it("does not synthesize a timeline item when the event omits item id", () => {
    applyStreamEvent(
      "sess-1",
      waitingForUserAction(1, pendingUserAction({ itemId: undefined })),
    );

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream?.pendingUserAction).toMatchObject({ actionId: "action-1" });
    expect(stream?.blocks).toEqual([]);
  });

  it("links resumed tool results to the original call id", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 10));
    applyStreamEvent("sess-1", waitingForUserAction(11));
    applyStreamEvent("sess-1", userInputStarted("resume-1", "approved", 12));
    applyStreamEvent("sess-1", toolResult("tool-1", "exit 0", 13));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find(
      (block) => block.kind === "timeline",
    );
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({
        itemId: "tool-1",
        callId: "call-tool-1",
        output: "exit 0",
        status: "completed",
      })],
    });
    expect(useAppStore.getState().streams["sess-1"]?.pendingUserAction).toBeUndefined();
  });

  it("continues rendering after a waiting turn resumes", () => {
    applyStreamEvent("sess-1", waitingForUserAction(1));
    applyStreamEvent("sess-1", userInputStarted("resume-1", "approved", 2));
    applyStreamEvent("sess-1", itemStarted("final-1", "assistant_text", "final_answer", 3));
    applyStreamEvent("sess-1", textDelta("final-1", "done", 4));

    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual(expect.arrayContaining([
      expect.objectContaining({
        kind: "timeline",
        items: expect.arrayContaining([expect.objectContaining({ text: "done" })]),
      }),
    ]));
  });
});
