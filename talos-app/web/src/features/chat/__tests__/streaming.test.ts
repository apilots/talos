import { beforeEach, describe, expect, it } from "vitest";

import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import {
  applyActiveTurnSnapshot,
  applyStreamEvent,
} from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import {
  activeTurn,
  assistantTextCommitted,
  contextCompactionDelta,
  contextCompactionStarted,
  executionMetrics,
  historyTextItem,
  historyToolItem,
  itemCompleted,
  itemStarted,
  sessionHistory,
  textDelta,
  toolProgress,
  toolResult,
  turnCompleted,
  turnMetrics,
  userInputStarted,
  withMessage,
} from "./streaming-fixtures";

describe("timeline stream events", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("streams explicit final answer deltas on the server timeline item", () => {
    applyStreamEvent("sess-1", itemStarted("commentary-1", "assistant_text", "commentary", 1));
    applyStreamEvent("sess-1", textDelta("commentary-1", "working", 2));
    applyStreamEvent("sess-1", itemStarted("final-1", "assistant_text", "final_answer", 3));
    applyStreamEvent("sess-1", textDelta("final-1", "done", 4));
    applyStreamEvent("sess-1", textDelta("final-1", " now", 5));

    const blocks = useAppStore.getState().streams["sess-1"]?.blocks ?? [];
    const timeline = blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({ itemId: "commentary-1", phase: "commentary", text: "working" }),
        expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "done now" }),
      ],
    });
  });

  it("keeps unknown assistant text on the same timeline item when committed as final", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "unknown", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "final text", 2));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 3));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 4));

    const blocks = useAppStore.getState().streams["sess-1"]?.blocks ?? [];
    expect(blocks).toEqual([
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "answer-1",
            sourceItemId: "answer-1",
            phase: "final_answer",
            text: "final text",
          }),
        ],
      }),
    ]);
  });

  it("continues appending final answer deltas after the server commits the phase", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "unknown", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "first", 2));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 3));
    applyStreamEvent("sess-1", textDelta("answer-1", " second", 4));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 5));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find(
      (block) => block.kind === "timeline",
    );
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "answer-1",
          sourceItemId: "answer-1",
          phase: "final_answer",
          text: "first second",
          status: "completed",
        }),
      ],
    });
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
    applyStreamEvent("sess-1", toolResult("tool-1", [
      {
        type: "web_reference",
        title: "Example",
        url: "https://example.com",
        citation_id: "web-1",
        snippet: "Example snippet",
        source_domain: "example.com",
        published_at: null,
        provider_rank: 1,
      },
    ], 3));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          kind: "tool_call",
          callId: "call-1",
          toolName: "Shell",
          status: "completed",
          output: "Example\nhttps://example.com\nExample snippet",
          progress: [{ kind: "status", message: "running" }],
        }),
      ],
    });
  });

  it("renders live context compaction timeline status", () => {
    applyStreamEvent("sess-1", contextCompactionStarted(
      "compact-1",
      "preparing",
      "Preparing session history compaction.",
      1,
    ));
    applyStreamEvent("sess-1", contextCompactionDelta(
      "compact-1",
      "completed",
      "applied",
      "Session history compaction finished.",
      2,
    ));
    applyStreamEvent("sess-1", itemCompleted("compact-1", 3));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "compact-1",
          kind: "context_compaction",
          phase: "system",
          status: "completed",
          text: "Session history compaction finished.",
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
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "answer-1",
            phase: "final_answer",
            text: "final text",
          }),
        ],
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

  it("merges deltas into the started reasoning item without leaving an empty timeline block", () => {
    applyStreamEvent("sess-1", itemStarted("reasoning-5", "reasoning", "thinking", 1));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "<think>", 2));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "\n", 3));
    applyStreamEvent("sess-1", itemCompleted("reasoning-5", 4));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      id: "timeline-sess-1-msg-1",
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "reasoning-5",
          sourceItemId: "reasoning-5",
          text: "<think>\n",
          status: "completed",
        }),
      ],
    });
    expect(timeline?.kind === "timeline" ? timeline.items : []).toHaveLength(1);
  });

  it("scopes repeated item ids by message when rendering live timeline blocks", () => {
    applyStreamEvent("sess-1", itemStarted("reasoning-5", "reasoning", "thinking", 1));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "first", 2));
    applyStreamEvent("sess-1", withMessage(itemStarted("reasoning-5", "reasoning", "thinking", 3), "msg-2", "turn-2"));
    applyStreamEvent("sess-1", withMessage(textDelta("reasoning-5", "second", 4), "msg-2", "turn-2"));

    const timelines = (useAppStore.getState().streams["sess-1"]?.blocks ?? []).filter((block) => block.kind === "timeline");
    expect(timelines).toEqual([
      expect.objectContaining({
        id: "timeline-sess-1-msg-2",
        items: [expect.objectContaining({ itemId: "reasoning-5", text: "second" })],
      }),
    ]);
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
      expect.objectContaining({
        kind: "timeline",
        items: [expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "done" })],
      }),
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

    expect(blocks.map((block) => block.kind)).toEqual(["user", "timeline"]);
    expect(blocks[1]).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({ itemId: "thinking-1", text: "I will inspect the module." }),
        expect.objectContaining({ itemId: "tool-1", kind: "tool_call", output: "lib.rs" }),
        expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "The module is layered." }),
      ],
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
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "final-1",
            phase: "final_answer",
            text: "direct answer",
          }),
        ],
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
      "timeline",
      "user",
      "timeline",
    ]);
    expect(blocks[1]).toMatchObject({
      kind: "timeline",
      live: false,
      items: [expect.objectContaining({ itemId: "final-history", text: "previous answer" })],
    });
    expect(blocks.at(-1)).toMatchObject({
      kind: "timeline",
      live: true,
      items: [expect.objectContaining({ itemId: "thinking-1", text: "Searching." })],
    });
  });
});
