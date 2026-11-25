import { beforeEach, expect, it } from "vitest";

import { useAppStore } from "@/shared/store/app-store";
import { applyStreamEvent, buildBlocksFromHistorySnapshot } from "../stream-events";
import { historyToolItems, itemStarted, sessionHistory, toolProgress } from "./streaming-fixtures";

const queued = "waiting for execution slot";
const legacyNotices = ["Executing Shell", "execution started"];

beforeEach(() => useAppStore.getState().resetStream("sess-1"));

it("keeps queue progress on a live tool without repeating the old start notices", () => {
  applyStreamEvent("sess-1", itemStarted("tool-1", "tool_call", "acting", 1));
  for (const [index, text] of [queued, ...legacyNotices].entries()) {
    applyStreamEvent("sess-1", toolProgress("tool-1", text, index + 2));
  }

  expect(useAppStore.getState().streams["sess-1"]?.blocks).toMatchObject([
    { kind: "timeline", items: [{ kind: "tool_call", progress: [{ kind: "status", message: queued }] }] },
  ]);
});

it("ignores only historical start notices, preserving queue status and same-text stdout", () => {
  const tools = historyToolItems("tool-1", "done");
  const call = tools[0]!.content;
  if (call.type !== "toolCall") throw new Error("Expected a tool call fixture");
  call.data.progress = [
    ...[queued, ...legacyNotices].map((text) => ({ kind: "status" as const, text, occurredAt: 1 })),
    { kind: "stdout", text: "execution started", occurredAt: 2 },
  ];

  expect(buildBlocksFromHistorySnapshot(sessionHistory(tools)).blocks).toMatchObject([
    { kind: "timeline", items: [{ kind: "tool_call", progress: [
      { kind: "status", message: queued },
      { kind: "stdout", message: "execution started" },
    ] }] },
  ]);
});
