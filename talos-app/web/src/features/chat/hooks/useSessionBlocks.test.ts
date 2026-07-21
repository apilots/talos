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
      clientMessageId: "client-1",
      phase: "local",
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
      expect.objectContaining({ id: "user-msg-1", text: "single input" }),
    ]);
  });
});
