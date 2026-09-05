import { beforeEach, describe, expect, it } from "vitest";

import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { applyStreamEvent } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import {
  historyUserInputItem,
  sessionHistory,
  userInputStarted,
} from "./streaming-fixtures";

describe("timeline attachment events", () => {
  beforeEach(() => {
    useAppStore.getState().resetStream("sess-1");
  });

  it("preserves attachment-only user input across live and restored history", () => {
    applyStreamEvent("sess-1", userInputStarted("input-1", "", 1, ["file-1"]));

    expect(useAppStore.getState().streams["sess-1"]?.blocks).toContainEqual(
      expect.objectContaining({
        kind: "user",
        text: "",
        fileIds: ["file-1"],
      }),
    );

    const restored = buildSessionBlocks({
      sessionId: "sess-1",
      sessionHistory: sessionHistory([
        ...historyUserInputItem("input-1", "", ["file-1"]),
      ]),
      waitingResponseDetail: "waiting detail",
      waitingResponseTitle: "waiting title",
      stoppingResponseDetail: "stopping detail",
      stoppingResponseTitle: "stopping title",
    });

    expect(restored).toContainEqual(
      expect.objectContaining({
        kind: "user",
        text: "",
        fileIds: ["file-1"],
      }),
    );
  });
});
