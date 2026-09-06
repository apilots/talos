import { beforeEach, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { applyStreamEvent } from "../stream-events";
import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { useAppStore } from "@/shared/store/app-store";
import { historyTextItem, sessionHistory, turnCompleted } from "./streaming-fixtures";

beforeEach(() => useAppStore.setState({ streams: {} }));

it("preserves success and deduplicates a late warning against restored history", () => {
  const record = {
    eventId: "ev_10", eventSeq: 10, persistedAt: 10,
    event: { type: "systemNotice", data: {
      identity: { sessionId: "sess-1", turnId: "turn-1" }, occurredAt: 10,
      notice: { level: "warning", content: { type: "error", data: {
        code: AppProtocol.ErrorCode.InternalError, message: "Usage could not be saved.", localization: null,
      } } },
    } },
  } satisfies AppProtocol.TimelineEvent;
  applyStreamEvent("sess-1", turnCompleted(9));
  const completed = useAppStore.getState().streams["sess-1"];
  applyStreamEvent("sess-1", record);
  applyStreamEvent("sess-1", record);
  const stream = useAppStore.getState().streams["sess-1"];
  const expectedNotice = { id: "notice-ev_10", kind: "notice", notice: record.event.data.notice, live: false };
  expect(stream.blocks.filter((block) => block.kind === "notice")).toEqual([expectedNotice]);
  expect({ running: stream.isStreaming, error: stream.error, turn: stream.activeTurnId })
    .toEqual({ running: completed.isStreaming, error: completed.error, turn: completed.activeTurnId });
  const history = sessionHistory([historyTextItem("answer", "assistant_text", "final_answer", "Saved answer")]);
  history.data[0].notices = [record];
  const blocks = buildSessionBlocks({ sessionId: "sess-1", streamState: stream, sessionHistory: history,
    waitingResponseTitle: "Waiting", waitingResponseDetail: "", stoppingResponseTitle: "Stopping", stoppingResponseDetail: "" });
  expect(blocks.filter((block) => block.kind === "notice")).toEqual([expectedNotice]);
  expect(JSON.stringify(blocks)).toContain("Saved answer");
  useAppStore.getState().resetLiveResponse("sess-1");
  expect(useAppStore.getState().streams["sess-1"].blocks).toEqual(blocks.filter((block) => block.kind === "notice"));
});
