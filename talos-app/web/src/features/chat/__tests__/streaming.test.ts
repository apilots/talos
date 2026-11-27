import { beforeEach, describe, expect, it } from "vitest";

import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { applyStreamEvent, buildBlocksFromHistorySnapshot } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import {
  assistantTextCommitted,
  executionMetrics,
  historyTextItem,
  historyToolItems,
  itemCompleted,
  itemStarted,
  sessionHistory,
  textDelta,
  toolExecutionMetrics,
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
        expect.objectContaining({ itemId: "commentary-1", phase: "commentary", text: "working" }) as unknown,
        expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "done now" }) as unknown,
      ],
    });
  });

  it("keeps unknown assistant text on the same timeline item when committed as final", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "pending", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "final text", 2));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 3));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 4));

    expect(useAppStore.getState().streams["sess-1"]?.blocks ?? []).toEqual([
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "answer-1",
            sourceItemId: "answer-1",
            phase: "final_answer", replayFinalAnswer: true,
            text: "final text",
          }) as unknown,
        ],
      }) as unknown,
    ]);
  });

  it("continues appending final answer deltas after the server commits the phase", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "pending", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "first", 2));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 3));
    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual([
      expect.objectContaining({ kind: "timeline", live: true }) as unknown,
    ]);
    applyStreamEvent("sess-1", textDelta("answer-1", " second", 4));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 5, "assistant_text", "finalAnswer"));

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
        }) as unknown,
      ],
    });
  });

  it("keeps unknown assistant text in the timeline when committed as commentary", () => {
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "pending", 1));
    applyStreamEvent("sess-1", textDelta("thinking-1", "checking", 2));
    applyStreamEvent("sess-1", assistantTextCommitted("thinking-1", "commentary", 3));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [expect.objectContaining({ itemId: "thinking-1", phase: "commentary", text: "checking" }) as unknown],
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
    ], 3, toolExecutionMetrics));

    const runningTimeline = useAppStore.getState().streams["sess-1"]?.blocks.find(
      (block) => block.kind === "timeline",
    );
    expect(runningTimeline).toMatchObject({
      items: [expect.objectContaining({ itemId: "tool-1", status: "completed", executionMetrics: toolExecutionMetrics }) as unknown],
    });

    applyStreamEvent("sess-1", itemCompleted("tool-1", 4, "tool_call"));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          kind: "tool_call",
          callId: "call-tool-1",
          toolName: "Shell",
          status: "completed",
          output: "Example\nhttps://example.com\nExample snippet",
          progress: [{ kind: "status", message: "running" }],
        }) as unknown,
      ],
    });
  });

  it("applies live turn metrics and advances the canonical event cursor", () => {
    applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
    applyStreamEvent("sess-1", turnMetrics(2, { modelRounds: 1, toolCalls: 1, total_tokens: 42 }));

    const stream = useAppStore.getState().streams["sess-1"];
    const timeline = stream?.blocks.find((block) => block.kind === "timeline");
    expect(stream?.lastEventSeq).toBe(2);
    expect(timeline).toMatchObject({
      kind: "timeline",
      metrics: expect.objectContaining({
        state: "running",
        modelRounds: 1,
        toolCalls: 1,
        usage: expect.objectContaining({ total_tokens: 42 }) as unknown,
      }) as unknown,
    });
  });

  it("keeps final metrics visible for a direct answer-only stream", () => {
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "pending", 1));
    applyStreamEvent("sess-1", textDelta("answer-1", "final text", 2));
    applyStreamEvent("sess-1", itemCompleted("answer-1", 3));
    applyStreamEvent("sess-1", assistantTextCommitted("answer-1", "final_answer", 4));
    applyStreamEvent("sess-1", turnMetrics(5, { total_tokens: 46 }));
    applyStreamEvent("sess-1", turnCompleted(6));

    expect(useAppStore.getState().streams["sess-1"]?.blocks ?? []).toEqual([
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "answer-1",
            phase: "final_answer",
            text: "final text",
          }) as unknown,
        ],
        metrics: expect.objectContaining({
          state: "completed",
          usage: expect.objectContaining({ total_tokens: 46 }) as unknown,
        }) as unknown,
      }) as unknown,
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
        expect.objectContaining({ kind: "assistant_text", text: "Before tool." }) as unknown,
        expect.objectContaining({ kind: "tool_call", itemId: "tool-1", output: "glob output" }) as unknown,
        expect.objectContaining({ kind: "assistant_text", text: "After first tool." }) as unknown,
        expect.objectContaining({ kind: "tool_call", itemId: "tool-2", output: "read output" }) as unknown,
        expect.objectContaining({ kind: "assistant_text", text: "After second tool." }) as unknown,
      ],
    });
  });

  it("records cursor from the envelope event id", () => {
    applyStreamEvent("sess-1", itemStarted("commentary-1", "assistant_text", "commentary", 7));

    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({
      lastEventId: "ev_7",
      lastSequence: 7,
      activeTurnId: "turn-1",
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
          completedAtMs: 9000,
          text: "working",
        }) as unknown,
      ],
    });
  });

  it("merges deltas into the started reasoning item without leaving an empty timeline block", () => {
    applyStreamEvent("sess-1", itemStarted("reasoning-5", "reasoning", "thinking", 1));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "<think>", 2));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "\n", 3));
    applyStreamEvent("sess-1", itemCompleted("reasoning-5", 4, "reasoning"));

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find((block) => block.kind === "timeline");
    expect(timeline).toMatchObject({
      id: "timeline-sess-1-turn-1",
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "reasoning-5",
          sourceItemId: "reasoning-5",
          text: "<think>\n",
          status: "completed",
        }) as unknown,
      ],
    });
    expect(timeline?.kind === "timeline" ? timeline.items : []).toHaveLength(1);
  });

  it("scopes repeated block ids by turn when rendering live timeline blocks", () => {
    applyStreamEvent("sess-1", itemStarted("reasoning-5", "reasoning", "thinking", 1));
    applyStreamEvent("sess-1", textDelta("reasoning-5", "first", 2));
    applyStreamEvent("sess-1", withMessage(itemStarted("reasoning-5", "reasoning", "thinking", 3), "msg-2", "turn-2"));
    applyStreamEvent("sess-1", withMessage(textDelta("reasoning-5", "second", 4), "msg-2", "turn-2"));

    const timelines = (useAppStore.getState().streams["sess-1"]?.blocks ?? []).filter((block) => block.kind === "timeline");
    expect(timelines).toEqual([
      expect.objectContaining({
        id: "timeline-sess-1-turn-2",
        items: [expect.objectContaining({ itemId: "reasoning-5", text: "second" }) as unknown],
      }) as unknown,
    ]);
  });

  it("rebuilds completed session history from stable history snapshot", () => {
    const history = sessionHistory([
      historyTextItem("input-1", "user_input", "input", "inspect prompt builder"),
      historyTextItem("thinking-1", "assistant_text", "commentary", "I will inspect the module."),
      ...historyToolItems("tool-1", "lib.rs", toolExecutionMetrics),
      historyTextItem("final-1", "assistant_text", "final_answer", "The module is layered."),
    ]);

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: undefined,
      history: buildBlocksFromHistorySnapshot(history),
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks.map((block) => block.kind)).toEqual(["user", "timeline"]);
    expect(blocks[1]).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({ itemId: "thinking-1", text: "I will inspect the module." }) as unknown,
        expect.objectContaining({ itemId: "tool-1", output: "lib.rs", startedAtMs: 1000, completedAtMs: 2000, executionMetrics: toolExecutionMetrics }) as unknown,
        expect.objectContaining({ itemId: "final-1", phase: "final_answer", text: "The module is layered." }) as unknown,
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
      history: buildBlocksFromHistorySnapshot(history),
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(blocks).toEqual([
      expect.objectContaining({ kind: "user", text: "hello" }) as unknown,
      expect.objectContaining({
        kind: "timeline",
        items: [
          expect.objectContaining({
            itemId: "final-1",
            phase: "final_answer",
            text: "direct answer",
          }) as unknown,
        ],
        metrics: expect.objectContaining({
          state: "completed",
          usage: expect.objectContaining({ total_tokens: 46 }) as unknown,
        }) as unknown,
      }) as unknown,
    ]);
  });

  it("renders live recovery from stream state without duplicating stable history", () => {
    const history = sessionHistory([
      historyTextItem("input-history", "user_input", "input", "previous question"),
      historyTextItem("final-history", "assistant_text", "final_answer", "previous answer"),
    ]);
    [
      userInputStarted("input-1", "run search", 1),
      itemCompleted("input-1", 2, "user_input"),
      itemStarted("thinking-1", "assistant_text", "commentary", 3),
      textDelta("thinking-1", "Searching.", 4),
    ].map((event) => withMessage(event, "msg-2", "turn-2")).forEach((event) => applyStreamEvent("sess-1", event));

    const blocks = buildSessionBlocks({
      sessionId: "sess-1",
      streamState: useAppStore.getState().streams["sess-1"],
      history: buildBlocksFromHistorySnapshot(history),
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
      items: [expect.objectContaining({ itemId: "final-history", text: "previous answer" }) as unknown],
    });
    expect(blocks.at(-1)).toMatchObject({
      kind: "timeline",
      live: true,
      items: [expect.objectContaining({ itemId: "thinking-1", text: "Searching." }) as unknown],
    });
  });

});
