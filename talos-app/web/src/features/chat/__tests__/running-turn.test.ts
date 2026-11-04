// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { applyStreamEventBatch, restoreActiveTurn } from "../stream-events";
import { createTimelineEventBatcher } from "../timeline-event-batcher";
import { pendingUserAction, textDelta, baseEvent } from "./streaming-fixtures";
import { runningTurn } from "./running-turn-fixture";

function answer() {
  return useAppStore.getState().streams["sess-1"]?.blocks
    .flatMap((block) => block.kind === "timeline" ? block.items : [])
    .filter((item) => item.kind !== "tool_call").map((item) => "text" in item ? item.text : "").join("");
}

describe("running Turn state and offset", () => {
  beforeEach(() => useAppStore.getState().resetStream("sess-1"));
  afterEach(() => vi.useRealTimers());

  it("restores one record, appends only later content and ignores duplicates", () => {
    const commits = vi.fn();
    const unsubscribe = useAppStore.subscribe(commits);
    restoreActiveTurn("sess-1", runningTurn());
    expect(commits).toHaveBeenCalledOnce();
    expect(answer()).toBe("base");
    const next = textDelta("answer", " tail", 10);
    applyStreamEventBatch("sess-1", [textDelta("answer", "old", 8), next, next]);
    expect(answer()).toBe("base tail");
    expect(useAppStore.getState().streams["sess-1"]).toMatchObject({ lastSequence: 10, lastEventSeq: 10, lastEventId: "ev_10" });
    expect(commits).toHaveBeenCalledTimes(2);
    restoreActiveTurn("sess-1", runningTurn());
    expect(answer()).toBe("base tail");
    unsubscribe();
  });

  it("restores tool arguments and a pending user action before further deltas", () => {
    const snapshot = runningTurn({ state: "waitingForInput", pendingInput: { type: "userAction", action: pendingUserAction() } });
    snapshot.messages[0].blocks = [{
      blockId: "tool-1", status: "waitingForInput", startedAt: 1, completedAt: null,
      content: { type: "toolCall", data: {
        callId: "call-tool-1", name: "Shell", arguments: { state: "streaming", jsonFragment: '{"x":' }, progress: [],
      } },
    }];
    restoreActiveTurn("sess-1", snapshot);
    expect(useAppStore.getState().streams["sess-1"]?.pendingUserAction).toEqual(snapshot.pendingInput?.type === "userAction" ? snapshot.pendingInput.action : undefined);
    applyStreamEventBatch("sess-1", [baseEvent({ type: "messageBlockDelta", data: {
      messageId: "msg-1", blockId: "tool-1", delta: { type: "toolCallArguments", data: { jsonFragment: "1}" } },
    } }, 10)]);
    const tools = useAppStore.getState().streams["sess-1"]?.blocks.flatMap((block) => block.kind === "timeline" ? block.items : []);
    expect(tools).toHaveLength(1);
    expect(tools?.[0]).toMatchObject({ kind: "tool_call", arguments: '{"x":1}' });
  });

  it("updates cancellation state at an unchanged content offset", () => {
    restoreActiveTurn("sess-1", runningTurn());
    restoreActiveTurn("sess-1", runningTurn({ state: "cancelling" }));
    expect(answer()).toBe("base");
    expect(useAppStore.getState().streams["sess-1"]?.connectionState).toBe("cancelling");
  });

  it("retains thousands of messages in one multi-hour Turn and continues its tail", () => {
    const snapshot = runningTurn({ lastSequence: 1_900_000, lastEventSeq: 2_000_000, lastEventId: "ev_2000000" });
    snapshot.messages.unshift(...Array.from({ length: 4000 }, (_, index) => ({
      messageId: `earlier-${index}`, turnId: "turn-1", role: "agent" as const,
      status: "completed" as const, createdAt: index, completedAt: index + 1,
      blocks: [{ blockId: `earlier-${index}`, status: "completed" as const,
        startedAt: index, completedAt: index + 1,
        content: { type: "reasoning" as const, data: { text: `Retained step ${index}` } } }],
    })));
    restoreActiveTurn("sess-1", snapshot);
    const next = textDelta("answer", "next", 1_900_001);
    next.eventSeq = 2_000_001;
    next.eventId = "ev_2000001";
    applyStreamEventBatch("sess-1", [next]);
    const items = useAppStore.getState().streams["sess-1"]?.blocks.flatMap((block) => block.kind === "timeline" ? block.items : []);
    expect(items).toHaveLength(4001);
    expect(items?.[0]).toMatchObject({ text: "Retained step 0" });
    expect(items?.[3999]).toMatchObject({ text: "Retained step 3999" });
    expect(items?.at(-1)).toMatchObject({ text: "basenext" });
  });

  it("yields between bounded batches and commits all burst text exactly once", () => {
    vi.useFakeTimers();
    restoreActiveTurn("sess-1", runningTurn());
    const commits = vi.fn();
    const unsubscribe = useAppStore.subscribe(commits);
    const batcher = createTimelineEventBatcher((events) => applyStreamEventBatch("sess-1", events), (error) => { throw error; });
    for (let seq = 10; seq < 1010; seq++) batcher.push(textDelta("answer", "x", seq));
    expect(answer()).toBe("basex");
    vi.advanceTimersByTime(16);
    expect(answer()).toBe("base" + "x".repeat(65));
    expect(commits).toHaveBeenCalledTimes(2);
    vi.runAllTimers();
    expect(answer()).toBe("base" + "x".repeat(1000));
    expect(commits.mock.calls.length).toBeLessThan(20);
    batcher.cancel(); unsubscribe();
  });

});
