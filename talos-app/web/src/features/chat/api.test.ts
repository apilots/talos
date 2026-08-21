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
      "user-1",
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
      session_id: "sess-1",
      input: [{ type: "text", text: "hello" }],
      modelSelection: {
        modelRef: { provider: "bailian", id: "qwen-max" },
        reasoningEffort: null,
      },
      userPermissionMode: "askForApproval",
      context: expect.objectContaining({
        user_id: "user-1",
        source_type: "http",
        provider: "sdk",
      }),
    });
  });

  it("keeps Skill mentions in text for backend resolution", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "user-1",
      "use $docs",
    );

    expect(request.input).toEqual([{ type: "text", text: "use $docs" }]);
  });

  it("keeps ordered file ids as one structured input item", () => {
    const request = createInitialTurnRequest(
      "sess-1",
      "user-1",
      "",
      { fileIds: ["file-2", "file-1"] },
    );

    expect(request.input).toEqual([
      { type: "files", fileIds: ["file-2", "file-1"] },
    ]);
  });
});

describe("chat message api", () => {
  afterEach(() => {
    resetTalosClientFactory();
  });

  it("returns the accepted message id without opening another event stream", async () => {
    const onAcceptedMessageId = vi.fn();
    const start = vi.fn(async () => ({
      request_id: "req-1",
      message_id: "msg-accepted",
      session_id: "sess-1",
      turn_id: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      turns: {
        start,
      },
    } as unknown as TalosWebClient));

    const messageId = await startTimelineTurn(
      { session_id: "sess-1", input: [{ type: "text", text: "hello" }] },
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
      request_id: "req-1",
      message_id: "msg-retry",
      session_id: "sess-1",
      turn_id: "turn-1",
      status: "accepted",
    }));
    setTalosClientFactory(() => ({
      turns: {
        retry,
      },
    } as unknown as TalosWebClient));

    const messageId = await retryTimelineTurn(
      {
        session_id: "sess-1",
        client_message_id: "client-1",
        retry_of_message_id: "msg-old",
        input: [{ type: "text", text: "hello" }],
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
