// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MessageQueue } from "@talos/sdk-browser";

import { useSessionMessageQueue } from "./useSessionMessageQueue";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

afterEach(resetTalosClientFactory);

describe("useSessionMessageQueue", () => {
  it("applies a successful mutation before the connection opens", async () => {
    const added: MessageQueue = {
      revision: 1,
      maxMessages: 10,
      messages: [{ id: "q1", input: { content: [{ type: "text", text: "waiting" }] } }],
    };
    const add = vi.fn().mockResolvedValue(added);
    setTalosClientFactory(() => ({
      messageQueue: { add },
    }) as unknown as TalosWebClient);

    const { result } = renderHook(() => useSessionMessageQueue("sess-1"));

    await act(async () => {
      await result.current.add({ content: [{ type: "text", text: "waiting" }] });
    });

    expect(result.current.queue).toEqual(added);
  });

  it("lists after subscription opens, clears mutation errors on change, and clears on disconnect", async () => {
    const queues: MessageQueue[] = [
      { revision: 1, maxMessages: 10, messages: [] },
      {
        revision: 2,
        maxMessages: 10,
        messages: [{ id: "q1", input: { content: [{ type: "text", text: "from another client" }] } }],
      },
    ];
    const list = vi.fn()
      .mockResolvedValueOnce(queues[0])
      .mockResolvedValueOnce(queues[1]);
    const deleteQueuedMessage = vi.fn().mockRejectedValue(new Error("busy"));
    setTalosClientFactory(() => ({
      messageQueue: { delete: deleteQueuedMessage, list },
    }) as unknown as TalosWebClient);

    const { result } = renderHook(() =>
      useSessionMessageQueue("sess-1"),
    );
    expect(list).not.toHaveBeenCalled();

    act(() => result.current.onOpen(vi.fn()));
    await waitFor(() => expect(result.current.queue?.revision).toBe(1));

    await act(async () => {
      await result.current.delete("q1").catch(() => undefined);
    });
    expect(result.current.error).toBe("busy");

    act(() => result.current.onMessageQueueChanged({ sessionId: "sess-1", revision: 2 }));
    await waitFor(() => expect(result.current.queue?.revision).toBe(2));
    expect(result.current.error).toBeUndefined();

    act(() => result.current.onDisconnect());
    await waitFor(() => expect(result.current.queue).toBeUndefined());
    expect(list).toHaveBeenCalledTimes(2);
  });
});
