import { describe, expect, it, vi } from "vitest";

import { TalosClient } from "../client";

describe("TalosClient message queue", () => {
  it("maps queue management operations to the shared HTTP resource", async () => {
    const request = vi.fn().mockResolvedValue({ revision: 0, maxMessages: 10, messages: [] });
    const client = new TalosClient(
      { request },
      { stream: vi.fn().mockResolvedValue(undefined) },
      "http://localhost:8080",
    );
    const input = { content: [{ type: "text" as const, text: "queued" }] };

    await client.messageQueue.add({ sessionId: "s1", input });
    await client.messageQueue.list({ sessionId: "s1" });
    await client.messageQueue.update({ sessionId: "s1", messageId: "q1", input });
    await client.messageQueue.delete({ sessionId: "s1", messageId: "q1" });
    await client.messageQueue.reorder({ sessionId: "s1", messageIds: ["q2", "q1"] });
    await client.messageQueue.start({ sessionId: "s1", messageId: null });

    expect(request.mock.calls.map(([url, options]) => [url, JSON.parse(String(options.body))]))
      .toEqual([
        ["http://localhost:8080/v1/message-queue/add", { sessionId: "s1", input }],
        ["http://localhost:8080/v1/message-queue/list", { sessionId: "s1" }],
        ["http://localhost:8080/v1/message-queue/update", { sessionId: "s1", messageId: "q1", input }],
        ["http://localhost:8080/v1/message-queue/delete", { sessionId: "s1", messageId: "q1" }],
        ["http://localhost:8080/v1/message-queue/reorder", { sessionId: "s1", messageIds: ["q2", "q1"] }],
        ["http://localhost:8080/v1/message-queue/start", { sessionId: "s1", messageId: null }],
      ]);
  });

  it("routes queue changes separately from timeline events", async () => {
    const onEvent = vi.fn();
    const onMessageQueueChanged = vi.fn();
    const stream = vi.fn().mockImplementation(async (_url, _options, onMessage) => {
      onMessage(JSON.stringify({ sessionId: "s1", revision: 4 }), "messageQueue/changed");
    });
    const client = new TalosClient({ request: vi.fn() }, { stream }, "http://localhost:8080");

    await client.turns.events("s1", undefined, onEvent, undefined, {
      persistent: true,
      onMessageQueueChanged,
    });

    expect(onMessageQueueChanged).toHaveBeenCalledWith({ sessionId: "s1", revision: 4 });
    expect(onEvent).not.toHaveBeenCalled();
  });
});
