import { beforeEach, describe, expect, it } from "vitest";

import { applyStreamEvent } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import { itemStarted, textDelta, userInputStarted } from "./streaming-fixtures";

describe("same-turn user input streaming", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("interleaves user inputs with model work", () => {
    applyStreamEvent("sess-1", userInputStarted("input-1", "initial", 1));
    applyStreamEvent("sess-1", itemStarted("thinking-1", "assistant_text", "commentary", 2));
    applyStreamEvent("sess-1", textDelta("thinking-1", "before steer", 3));
    applyStreamEvent("sess-1", userInputStarted("input-2", "steer", 4, ["file-1"]));
    applyStreamEvent("sess-1", itemStarted("answer-1", "assistant_text", "final_answer", 5));
    applyStreamEvent("sess-1", textDelta("answer-1", "after steer", 6));

    expect(useAppStore.getState().streams["sess-1"]?.blocks).toEqual([
      expect.objectContaining({ id: "user-msg-1-input-1", text: "initial", fileIds: [] }),
      expect.objectContaining({
        id: "timeline-sess-1-msg-1-input-1",
        items: [expect.objectContaining({ text: "before steer" })],
      }),
      expect.objectContaining({ id: "user-msg-1-input-2", text: "steer", fileIds: ["file-1"] }),
      expect.objectContaining({
        id: "timeline-sess-1-msg-1-input-2",
        items: [expect.objectContaining({ text: "after steer" })],
      }),
    ]);
  });
});
