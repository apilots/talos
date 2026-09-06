import { afterEach, describe, expect, it, vi } from "vitest";

import {
  createInitialTurnRequest,
  retryTimelineTurn,
  startTimelineTurn,
  subscribeTimelineEvents,
} from "./api";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

describe("chat api initial turn request", () => {
  it("builds first-turn metadata with the selected permission mode", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "hello",
      {
        modelSelection: {
          modelRef: { provider: "bailian", id: "qwen-max" },
          reasoningEffort: null,
        },
        userPermissionMode: "askForApproval",
      },
    );

    expect(request).toMatchObject({
      sessionId: "sess-1",
      input: { content: [{ type: "text", text: "hello" }] },
      modelSelection: {
        modelRef: { provider: "bailian", id: "qwen-max" },
        reasoningEffort: null,
      },
      userPermissionMode: "askForApproval",
    });
  });

  it("keeps Skill mentions in text for backend resolution", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "use $docs",
    );

    expect(request.input).toEqual({ content: [{ type: "text", text: "use $docs" }] });
  });

  it("keeps ordered file ids as one structured input item", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "",
      { fileIds: ["file-2", "file-1"] },
    );

    expect(request.input).toEqual({
      content: [
        { type: "file", fileId: "file-2" },
        { type: "file", fileId: "file-1" },
      ],
    });
  });
});

describe("chat message api", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("returns the accepted message id without opening another event stream", async () => {
    const onAcceptedMessageId = vi.fn();
    const start = vi.fn(async () => ({
      requestId: "req-1",
      messageId: "msg-accepted",
      sessionId: "sess-1",
      turnId: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      turns: {
        start,
      },
    } as unknown as TalosWebClient));

    const messageId = await startTimelineTurn(
      {
        sessionId: "sess-1",
        input: { content: [{ type: "text", text: "hello" }] },
        modelSelection: null,
        userPermissionMode: null,
        projectId: null,
        currentDirectory: null,
        clientRequestId: null,
      },
      onAcceptedMessageId,
    );

    expect([messageId, onAcceptedMessageId.mock.calls]).toEqual([
      "msg-accepted",
      [["msg-accepted"]],
    ]);
  });

  it("returns the accepted retry message id without opening another event stream", async () => {
    const onAcceptedMessageId = vi.fn();
    const retry = vi.fn(async () => ({
      requestId: "req-1",
      messageId: "msg-retry",
      sessionId: "sess-1",
      turnId: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      turns: {
        retry,
      },
    } as unknown as TalosWebClient));

    const messageId = await retryTimelineTurn(
      {
        sessionId: "sess-1",
        clientMessageId: "client-1",
        retryOfMessageId: "msg-old",
        input: { content: [{ type: "text", text: "hello" }] },
      },
      onAcceptedMessageId,
    );

    expect([messageId, onAcceptedMessageId.mock.calls]).toEqual([
      "msg-retry",
      [["msg-retry"]],
    ]);
  });
});
  it("requests a persistent session timeline subscription", async () => {
    const events = vi.fn().mockResolvedValue(undefined);
    const signal = new AbortController().signal;
    const onEvent = vi.fn();
    setTalosClientFactory(() => ({ turns: { events } }) as unknown as TalosWebClient);

    await subscribeTimelineEvents("sess-1", onEvent, signal, "event-4");

    expect(events).toHaveBeenCalledWith(
      "sess-1",
      "event-4",
      onEvent,
      signal,
      { persistent: true },
    );
  });
