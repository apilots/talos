import { beforeEach, describe, expect, it } from "vitest";

import { applyStreamEvent } from "../stream-events";
import { activeContextCompactionStage } from "../timeline-state";
import { useAppStore } from "@/shared/store/app-store";
import {
  contextCompactionDelta,
  contextCompactionStarted,
  itemCompleted,
  turnCompleted,
} from "./streaming-fixtures";

describe("context compaction stream state", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("keeps the compaction guard active independently of text streaming", () => {
    applyStreamEvent("sess-1", contextCompactionStarted(
      "compact-1",
      "preparing",
      "Preparing session history compaction.",
      1,
    ));

    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBe("preparing");
    useAppStore.getState().setStreamMeta("sess-1", { isStreaming: false });
    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBe("preparing");

    applyStreamEvent("sess-1", contextCompactionDelta(
      "compact-1",
      "completed",
      null,
      "Session history compaction finished.",
      2,
    ));
    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBeUndefined();
  });

  it("clears the compaction guard on a terminal turn event", () => {
    applyStreamEvent("sess-1", contextCompactionStarted(
      "compact-1",
      "trimming",
      "Trimming context.",
      1,
    ));
    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBe("trimming");

    applyStreamEvent("sess-1", turnCompleted(2, 0));

    expect(activeContextCompactionStage(useAppStore.getState().streams["sess-1"])).toBeUndefined();
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

    const timeline = useAppStore.getState().streams["sess-1"]?.blocks.find(
      (block) => block.kind === "timeline",
    );
    expect(timeline).toMatchObject({
      kind: "timeline",
      items: [
        expect.objectContaining({
          itemId: "compact-1",
          kind: "context_compaction",
          phase: "system",
          status: "completed",
          stage: "completed",
          text: "Session history compaction finished.",
        }),
      ],
    });
  });
});
