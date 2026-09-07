import { beforeEach, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-core";
import { applyStreamEvent, buildBlocksFromHistorySnapshot } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import { executionMetrics, turnCompleted } from "./streaming-fixtures";

beforeEach(() => useAppStore.setState({ streams: {} }));

it("retains the same public error in live state and historical timeline", () => {
  const error: AppProtocol.ApiError = {
    code: AppProtocol.ErrorCode.InternalError,
    message: "The response stream ended before a terminal frame arrived.", localization: null,
  };
  const record = turnCompleted(1);
  record.event.data.payload = { type: "turnFailed", data: { error, retryable: false } };
  applyStreamEvent("sess-1", record);
  const history = buildBlocksFromHistorySnapshot({ data: [{
    notices: [], identity: record.event.data.identity, completedEventSeq: 1, completedAt: 1,
    result: { status: "failed", result: {
      messages: [], metrics: executionMetrics(), failure: { error, retryable: false },
    } },
  }], nextCursor: null });
  expect({ live: useAppStore.getState().streams["sess-1"].error,
    history: history.blocks.filter((block) => block.kind === "timeline").map((block) => block.error),
  }).toEqual({ live: error, history: [error] });
});
