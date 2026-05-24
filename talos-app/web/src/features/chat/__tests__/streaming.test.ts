import { beforeEach, describe, expect, it } from "vitest";
import type {
  TalosActiveTurn,
  TalosSessionHistorySnapshot,
  TalosTimelineEvent,
  TimelineEventEnvelope,
} from "@talos/sdk-core";

import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import {
  applyActiveTurnSnapshot,
  applyStreamEvent,
} from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";

describe("timeline stream events", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("keeps final answer separate from commentary", () => {
    applyStreamEvent("sess-1", itemStarted("commentary-1", "assistant_text", "commentary", 1));
    applyStreamEvent("sess-1", textDelta("commentary-1", "working", 2));
    applyStreamEvent("sess-1", itemStarted("final-1", "assistant_text", "final_answer", 3));
    applyStreamEvent("sess-1", textDelta("final-1", "done", 4));

    const blocks = useAppStore.getState().streams["sess-1"]?.blocks ?? [];
    const timeline = blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({ itemId: "commentary-1", phase: "commentary", text: "working" })],
    });
    expect(blocks).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "assistant", text: "done" }),
    ]));
  });

  it("moves unknown assistant text to the answer block when committed as final", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "unknown", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "final text", 2));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 3));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 4));

    const blocks = useAppStore.getState().streams["sess-1"]?.blocks ?? [];
    expect(blocks.find((block) => block.kind === "timeline")).toBeUndefined();
    expect(blocks).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "assistant", text: "final text" }),
    ]));
  });

  it("keeps unknown assistant text in the timeline when committed as commentary", () => {
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "unknown", 1));
    applyStreamEvent("sess-1", textDelta("thinking-1", "checking", 2));
    applyStreamEvent("sess-1", assistantTextCommitted("thinking-1", "commentary", 3));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({ itemId: "thinking-1", phase: "commentary", text: "checking" })],
    });
  });

  it("updates tool progress on the same tool block", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
    applyStreamEvent("sess-1", toolProgress("tool-1", "running", 2));
    applyStreamEvent("sess-1", toolResult("tool-1", "ok", 3));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          kind: "tool_call",
          callId: "call-1",
          toolName: "Shell",
          status: "completed",
          output: "ok",
          progress: [{ kind: "status", message: "running" }],
        }),
      ],
    });
  });

  it("applies live turn metrics to the timeline block without advancing stream cursor", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
    applyStreamEvent("sess-1", turnMetrics(2, { model_rounds: 1, tool_calls: 1, total_tokens: 42 }));

    const stream = useAppStore.getState().streams["sess-1"];
    const timeline = stream?.blocks.find((block) => block.kind === "timeline");
    expect(stream?.lastEventSeq).toBe(1);
    expect(timeline).toMatchObject({
      kind: "timeline",
      metrics: expect.objectContaining({
        state: "running",
        model_rounds: 1,
        tool_calls: 1,
        usage: expect.objectContaining({ total_tokens: 42 }),
      }),
    });
  });

  it("keeps final metrics visible for a direct answer-only stream", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "unknown", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "final text", 2));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 3));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 4));
    applyStreamEvent("sess-1", turnCompleted(5, 46));

    const blocks = useAppStore.getState().streams["sess-1"]?.blocks ?? [];
    expect(blocks).toEqual([
      expect.objectContaining({ kind: "assistant", text: "final text" }),
      expect.objectContaining({
        kind: "timeline",
        items: [],
        metrics: expect.objectContaining({
          state: "completed",
          usage: expect.objectContaining({ total_tokens: 46 }),
        }),
      }),
    ]);
  });

  it("renders text deltas in server event order around tool calls", () => {
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 1));
    applyStreamEvent("sess-1", textDelta("thinking-1", "Before tool.", 2));
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 3));
    applyStreamEvent("sess-1", toolResult("tool-1", "glob output", 4));
    applyStreamEvent("sess-1", textDelta("thinking-1", "After first tool.", 5));
    applyStreamEvent("sess-1", itemStarted("tool-2", "tool_call", "acting", 6));
    applyStreamEvent("sess-1", toolResult("tool-2", "read output", 7));
    applyStreamEvent("sess-1", textDelta("thinking-1", "After second tool.", 8));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({ kind: "assistant_text", text: "Before tool." }),
        expect.objectContaining({ kind: "tool_call", itemId: "tool-1", output: "glob output" }),
        expect.objectContaining({ kind: "assistant_text", text: "After first tool." }),
        expect.objectContaining({ kind: "tool_call", itemId: "tool-2", output: "read output" }),
        expect.objectContaining({ kind: "assistant_text", text: "After second tool." }),
      ],
    });
  });

  it("does not show the waiting placeholder once timeline events are rendering", () => {
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 1));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.map((block) => block.kind)).toEqual(["timeline"]);
  });

  it("renders server user input without duplicating completed pending input", () => {
    useAppStore.getState().setPendingInput("sess-1", {
      text: "hello from local",
      clientMessageId: "client-1",
      phase: "local",
    });

    applyStreamEvent("sess-1", userInputStarted("input-1", "hello from server", 1));
    applyStreamEvent("sess-1", itemCompleted("input-1", 2));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.filter((block) => block.kind === "user")).toEqual([
      expect.objectContaining({ id: "user-msg-1", text: "hello from server" }),
    ]);
    expect(useAppStore.getState().streams["sess-1"]?.pendingInput?.phase).toBe("completed");
  });

  it("records cursor from the envelope event id", () => {
    applyStreamEvent("sess-1", itemStarted("commentary-1", "assistant_text", "commentary", 7));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_7",
      lastSequence: 7,
      activeMessageId: "msg-1",
      connectionState: "streaming",
    });
  });

  it("updates timeline item status and completion timestamp from item completed", () => {
    applyStreamEvent("sess-1", itemStarted("commentary-1", "assistant_text", "commentary", 1));
    applyStreamEvent("sess-1", textDelta("commentary-1", "working", 2));
    applyStreamEvent("sess-1", itemCompleted("commentary-1", 9));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "commentary-1",
          status: "completed",
          completedAtMs: 9,
          text: "working",
        }),
      ],
    });
  });

  it("preserves active turn cursor while reconnecting running sessions", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "running",
      message_id: "msg-1",
      last_event_id: "ev_9",
    }));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      activeMessageId: "msg-1",
      lastEventId: "ev_9",
      connectionState: "detached",
      isStreaming: true,
    });

    applyStreamEvent("sess-1", itemStarted("final-1", "assistant_text", "final_answer", 10));
    applyStreamEvent("sess-1", textDelta("final-1", "done", 11));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_11",
      connectionState: "streaming",
    });
    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual(expect.arrayContaining([
      expect.objectContaining({ kind: "assistant", text: "done" }),
    ]));
  });

  it("restores pending dynamic tool result on the original timeline item id", () => {
    applyActiveTurnSnapshot("sess-1", activeTurn({
      status: "waiting_user_action",
      pending_user_action: {
        type: "dynamic_tool_result",
        item_id: "tool-1",
        call_id: "call-1",
        tool_name: "DynamicTool",
      },
    }));
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 10));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "tool-1",
          callId: "call-1",
          status: "running",
        }),
      ],
    });
    expect(timeline?.kind === "timeline" ? timeline.items : []).toHaveLength(1);
  });

  it("rebuilds completed session history from stable history snapshot", () => {
    const history = sessionHistory([
      historyTextItem("input-1", "user_input", "input", "inspect prompt builder"),
      historyTextItem("thinking-1", "assistant_text", "commentary", "I will inspect the module."),
      historyToolItem("tool-1", "lib.rs"),
      historyTextItem("final-1", "assistant_text", "final_answer", "The module is layered."),
    ]);

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: undefined,
      sessionHistory: history,
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.map((block) => block.kind)).toEqual(["user", "timeline", "assistant"]);
    expect(blocks[1]).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({ itemId: "thinking-1", text: "I will inspect the module." }),
        expect.objectContaining({ itemId: "tool-1", kind: "tool_call", output: "lib.rs" }),
      ],
    });
    expect(blocks[2]).toMatchObject({
      kind: "assistant",
      text: "The module is layered.",
    });
  });

  it("rebuilds final-answer-only history with final metrics", () => {
    const history = sessionHistory(
      [
        historyTextItem("input-1", "user_input", "input", "hello"),
        historyTextItem("final-1", "assistant_text", "final_answer", "direct answer"),
      ],
      executionMetrics(46),
    );

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: undefined,
      sessionHistory: history,
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks).toEqual([
      expect.objectContaining({ kind: "user", text: "hello" }),
      expect.objectContaining({ kind: "assistant", text: "direct answer" }),
      expect.objectContaining({
        kind: "timeline",
        items: [],
        metrics: expect.objectContaining({
          state: "completed",
          usage: expect.objectContaining({ total_tokens: 46 }),
        }),
      }),
    ]);
  });

  it("renders live recovery from stream state without duplicating stable history", () => {
    const history = sessionHistory([
      historyTextItem("input-history", "user_input", "input", "previous question"),
      historyTextItem("final-history", "assistant_text", "final_answer", "previous answer"),
    ]);
    [
      userInputStarted("input-1", "run search", 1),
      itemCompleted("input-1", 2),
      itemStarted("thinking-1", "assistant_text", "commentary", 3),
      textDelta("thinking-1", "Searching.", 4),
    ].forEach((event) => applyStreamEvent("sess-1", event));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      sessionHistory: history,
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.map((block) => block.kind)).toEqual([
      "user",
      "assistant",
      "user",
      "timeline",
    ]);
    expect(blocks[1]).toMatchObject({ text: "previous answer", live: false });
    expect(blocks.at(-1)).toMatchObject({
      kind: "timeline",
      live: true,
      items: [expect.objectContaining({ itemId: "thinking-1", text: "Searching." })],
    });
  });
});

function baseEvent(payload: TalosTimelineEvent["payload"], sequence: number): TimelineEventEnvelope {
  return {
    event_seq: sequence,
    event_id: `ev_${sequence}`,
    created_at_ms: sequence,
    turn_id: "turn-1",
    message_id: "msg-1",
    session_id: "sess-1",
    event: {
      event_id: `ev_${sequence}`,
      event_seq: sequence,
      created_at_ms: sequence,
      event_type: payload.type,
      turn_id: "turn-1",
      message_id: "msg-1",
      session_id: "sess-1",
      sequence,
      payload,
    },
  };
}

function itemStarted(
  itemId: string,
  kind: "assistant_text" | "tool_call" | "user_input",
  phase: "commentary" | "final_answer" | "unknown" | "acting" | "input",
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "item_started",
    request_id: "req-1",
    turn_id: "turn-1",
    item: {
      item_id: itemId,
      parent_item_id: null,
      kind,
      phase,
      status: "running",
      title: kind === "tool_call" ? "Shell" : null,
      started_at_ms: 1n,
      completed_at_ms: null,
      content: kind === "tool_call"
        ? {
            type: "tool_call",
            call_id: "call-1",
            name: "Shell",
            arguments: "{}",
            progress: [],
            result: null,
          }
        : { type: "text", text: "" },
    },
  }, sequence);
}

function assistantTextCommitted(
  itemId: string,
  phase: "commentary" | "final_answer",
  sequence: number,
): TimelineEventEnvelope {
  return baseEvent({
    type: "assistant_text_committed",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    phase,
  }, sequence);
}

function userInputStarted(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  const event = itemStarted(itemId, "user_input", "input", sequence);
  if (event.event.payload.type === "item_started") {
    event.event.payload.item.content = { type: "text", text };
  }
  return event;
}

function itemCompleted(itemId: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_completed",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    status: "completed",
    completed_at_ms: BigInt(sequence),
    final_content: null,
  }, sequence);
}

function textDelta(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: { type: "text", text },
  }, sequence);
}

function toolProgress(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: { type: "tool_progress", progress_kind: "status", text_delta: text },
  }, sequence);
}

function toolResult(itemId: string, text: string, sequence: number): TimelineEventEnvelope {
  return baseEvent({
    type: "item_delta",
    request_id: "req-1",
    turn_id: "turn-1",
    item_id: itemId,
    delta: {
      type: "tool_result",
      content: [{ type: "text", text }],
      is_error: false,
    },
  }, sequence);
}

function turnMetrics(
  sequence: number,
  overrides: Partial<{
    model_rounds: number;
    tool_calls: number;
    tool_successes: number;
    tool_errors: number;
    total_tokens: number;
  }> = {},
): TimelineEventEnvelope {
  return baseEvent({
    type: "turn_metrics",
    request_id: "req-1",
    turn_id: "turn-1",
    metrics: {
      model_rounds: overrides.model_rounds ?? 0,
      tool_calls: overrides.tool_calls ?? 0,
      tool_successes: overrides.tool_successes ?? 0,
      tool_errors: overrides.tool_errors ?? 0,
      stop_reason: "end_turn",
      usage: {
        ...usage(),
        total_tokens: overrides.total_tokens ?? 0,
      },
      duration_ms: null,
    },
  }, sequence);
}

function turnCompleted(sequence: number, totalTokens = 0): TimelineEventEnvelope {
  return baseEvent({
    type: "turn_completed",
    request_id: "req-1",
    turn_id: "turn-1",
    completed_at_ms: sequence,
    stop_reason: "end_turn",
    usage: { ...usage(), total_tokens: totalTokens },
    metrics: executionMetrics(totalTokens),
  }, sequence);
}

function sessionHistory(
  items: TalosSessionHistorySnapshot["messages"][number]["items"],
  metrics: TalosSessionHistorySnapshot["messages"][number]["metrics"] = null,
): TalosSessionHistorySnapshot {
  return {
    session_id: "sess-1",
    messages: [
      {
        message_id: "msg-1",
        turn_id: "turn-1",
        items,
        stop_reason: "end_turn",
        usage: usage(),
        metrics,
      },
    ],
    generated_at_ms: 1,
    last_stable_event_id: "ev_12",
  };
}

function historyTextItem(
  itemId: string,
  kind: "assistant_text" | "user_input",
  phase: "commentary" | "final_answer" | "unknown" | "input",
  text: string,
): TalosSessionHistorySnapshot["messages"][number]["items"][number] {
  return {
    item_id: itemId,
    parent_item_id: null,
    kind,
    phase,
    status: "completed",
    title: null,
    started_at_ms: 1,
    completed_at_ms: 2,
    content: { type: "text", text },
  };
}

function historyToolItem(
  itemId: string,
  output: string,
): TalosSessionHistorySnapshot["messages"][number]["items"][number] {
  return {
    item_id: itemId,
    parent_item_id: null,
    kind: "tool_call",
    phase: "acting",
    status: "completed",
    title: "Shell",
    started_at_ms: 1,
    completed_at_ms: 2,
    content: {
      type: "tool_call",
      call_id: "call-1",
      name: "Shell",
      arguments: "{}",
      progress: [],
      result: {
        content: [{ type: "text", text: output }],
        is_error: false,
      },
    },
  };
}

function executionMetrics(totalTokens = 0) {
  return {
    model_rounds: 1,
    tool_calls: 0,
    tool_successes: 0,
    tool_errors: 0,
    stop_reason: "end_turn" as const,
    usage: { ...usage(), total_tokens: totalTokens },
    duration_ms: null,
  };
}

function usage() {
  return {
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
    cache_creation_input_tokens: null,
    cache_read_input_tokens: null,
  };
}

function activeTurn(overrides: Partial<TalosActiveTurn>): TalosActiveTurn {
  return {
    session_id: "sess-1",
    turn_id: "turn-1",
    message_id: "msg-1",
    status: "running",
    last_event_id: null,
    pending_user_action: null,
    updated_at: 1,
    ...overrides,
  };
}
