import { beforeEach, describe, expect, it } from "vitest";

import {
  applyActiveTurnSnapshot,
  applyStreamEvent,
  applyTimelineRecovery,
} from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import {
  activeTurn,
  baseEvent,
  itemCompleted,
  itemStarted,
  textDelta,
  toolResult,
  turnCompleted,
  withMessage,
} from "./streaming-fixtures";

describe("timeline user action events", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("keeps the live turn active while the backend waits for user action", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
    applyStreamEvent("sess-1", turnCompleted(2, 46, "waiting_user_action"));

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream).toMatchObject({
      activeMessageId: "msg-1",
      isStreaming: true,
      connectionState: "streaming",
      lastEventId: "ev_2",
    });
    expect(stream?.blocks).toEqual([
      expect.objectContaining({
        kind: "timeline",
        live: true,
        metrics: expect.objectContaining({
          state: "running",
          stop_reason: "waiting_user_action",
        }),
      }),
    ]);

    applyStreamEvent("sess-1", turnCompleted(3, 52));
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      isStreaming: false,
      connectionState: "idle",
      lastEventId: "ev_3",
    });
  });


  it("does not advance the consumed cursor from an active turn snapshot", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      lastEventId: "ev_7",
    });
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "running",
      message_id: "msg-1",
      last_event_id: "ev_9",
    }));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeTurnId: "turn-1",
      activeMessageId: "msg-1",
      lastEventId: "ev_7",
      connectionState: "detached",
      isStreaming: true,
    });

    applyStreamEvent("sess-1", itemStarted("final-1", "assistant_text", "final_answer", 10));
    applyStreamEvent("sess-1", textDelta("final-1", "done", 11));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeTurnId: "turn-1",
      lastEventId: "ev_11",
      connectionState: "streaming",
    });
    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual(expect.arrayContaining([
      expect.objectContaining({
        kind: "timeline",
        items: [expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "done" })],
      }),
    ]));
  });

  it("restores a cancelling turn without treating it as completed", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "cancelling",
      message_id: "msg-1",
    }));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeTurnId: "turn-1",
      activeMessageId: "msg-1",
      isStreaming: true,
      connectionState: "cancelling",
    });
  });

  it("does not rewind a newer consumed cursor to a recovery high-water mark", () => {
    useAppStore.getState().setStreamMeta("sess-1", {
      activeMessageId: "msg-1",
      isStreaming: true,
      lastEventId: "ev_12",
      lastEventSeq: 12,
    });

    applyTimelineRecovery("sess-1", {
      replay: {
        events: [],
        high_water_mark_event_id: "ev_9",
        last_replay_seq: 9,
        cursor_status: "from_beginning",
      },
      active_turn: activeTurn({
        status: "running",
        last_event_id: "ev_9",
      }),
    });

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_12",
      lastEventSeq: 12,
    });
  });

  it("uses the latest timeline turn identity for user-action submission", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "completed",
      turn_id: "turn-old",
    }));
    applyStreamEvent(
      "sess-1",
      withMessage(itemStarted("tool-1", "tool_call", "acting", 10), "msg-new", "turn-new"),
    );

    expect(useAppStore.getState().streams["sess-1"]?.activeTurnId).toBe("turn-new");
  });

  it("restores pending dynamic tool result on the original timeline item id", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 10));
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "waiting_user_action",
      pending_user_action: {
        actionId: "action-1",
        itemId: "tool-1",
        callId: "call-1",
        state: "pending",
        params: {
          type: "dynamicToolResult",
          params: { toolName: "DynamicTool" },
        },
      },
    }));
    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "tool-1",
          callId: "call-1",
          status: "waiting_user_action",
        }),
      ],
    });
    expect(timeline?.kind === "timeline" ? timeline.items : []).toHaveLength(1);
  });

  it("does not synthesize a timeline identity when the backend omits item id", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "waiting_user_action",
      pending_user_action: {
        actionId: "action-without-item",
        callId: "call-without-item",
        state: "pending",
        params: {
          type: "dynamicToolResult",
          params: { toolName: "DynamicTool" },
        },
      },
    }));

    const stream = useAppStore.getState().streams["sess-1"];
    expect(stream?.pendingUserAction).toMatchObject({ actionId: "action-without-item" });
    expect(stream?.blocks).toEqual([]);
  });

  it("applies resumed tool results to the original item after waiting for approval", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "waiting_user_action",
      pending_user_action: {
        actionId: "action-1",
        itemId: "tool-1",
        callId: "call-1",
        state: "pending",
        params: {
          type: "sandboxApproval",
          params: {
            toolName: "Shell",
            reason: "fileSystemAccessOutsideWorkspace",
            presentation: {
              type: "command",
              command: "python3 script.py",
              cwd: "/workspace",
            },
            availableDecisions: ["allowOnce", "deny", "cancel"],
          },
        },
      },
    }));
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 10));
    applyStreamEvent("sess-1", baseEvent({
      type: "turn_completed",
      request_id: "req-1",
      turn_id: "turn-1",
      completed_at_ms: 11,
      stop_reason: "waiting_user_action",
      usage: {
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
        cache_creation_input_tokens: null,
        cache_read_input_tokens: null,
      },
      metrics: null,
    }, 11));
    applyStreamEvent("sess-1", baseEvent({
      type: "turn_started",
      request_id: "req-2",
      turn_id: "turn-1",
      started_at_ms: 12,
    }, 12));
    applyStreamEvent("sess-1", toolResult("tool-1", "exit 0", 13));
    applyStreamEvent("sess-1", itemCompleted("tool-1", 14));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find(
      (block) => block.kind === "timeline",
    );
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({
        itemId: "tool-1",
        callId: "call-1",
        output: "exit 0",
        status: "completed",
      })],
    });
    expect(timeline?.kind === "timeline" ? timeline.items : []).toHaveLength(1);
  });

});
