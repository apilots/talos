// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it } from "vitest";
import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { AppProtocol } from "@talos/sdk-core";
import { applyStreamEvent } from "../stream-events";
import { buildSessionBlocks } from "../hooks/useSessionBlocks";
import { useAppStore } from "@/shared/store/app-store";
import { MessageList } from "@/widgets/message-list/MessageList";
import { historyTextItem, sessionHistory, turnCompleted } from "./streaming-fixtures";

beforeEach(() => useAppStore.setState({ streams: {} }));
afterEach(cleanup);

it("preserves success and a warning's historical position across later Turns", () => {
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
  const later = sessionHistory([historyTextItem("later-answer", "assistant_text", "final_answer", "Later answer")]).data[0];
  later.identity.turnId = "turn-2";
  later.completedEventSeq = 20;
  for (const message of later.result.result.messages) {
    message.turnId = "turn-2";
    message.messageId = `later-${message.messageId}`;
  }
  history.data.push(later);
  applyStreamEvent("sess-1", { ...record, eventId: "ev_21", eventSeq: 21, persistedAt: 21 });
  const blocks = buildSessionBlocks({ sessionId: "sess-1", streamState: useAppStore.getState().streams["sess-1"], sessionHistory: history,
    waitingResponseTitle: "Waiting", waitingResponseDetail: "", stoppingResponseTitle: "Stopping", stoppingResponseDetail: "" });
  expect(blocks.filter((block) => block.kind === "notice")).toEqual([expectedNotice, { ...expectedNotice, id: "notice-ev_21" }]);
  render(createElement(MessageList, { blocks, emptyTitle: "Empty", emptyDetail: "" }));
  expect(screen.getAllByText(/^(Saved answer|Later answer|Usage could not be saved\.)$/)
    .map((element) => element.textContent)).toMatchInlineSnapshot(`
      [
        "Saved answer",
        "Usage could not be saved.",
        "Later answer",
        "Usage could not be saved.",
      ]
    `);
  useAppStore.getState().resetLiveResponse("sess-1");
  expect(useAppStore.getState().streams["sess-1"].blocks).toEqual(blocks.filter((block) => block.kind === "notice"));
});
