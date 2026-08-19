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
  it("applies a successful mutation before the subscription is enabled", async () => {
    const added: MessageQueue = {
      revision: 1,
      maxMessages: 10,
      messages: [{ id: "q1", input: [{ type: "text", text: "waiting" }] }],
    };
    const add = vi.fn().mockResolvedValue(added);
    setTalosClientFactory(() => ({
      messageQueue: { add },
    }) as unknown as TalosWebClient);

    const { result } = renderHook(() => useSessionMessageQueue("sess-1", false));

    await act(async () => {
      await result.current.add([{ type: "text", text: "waiting" }]);
    });

    expect(result.current.queue).toEqual(added);
  });

  it("lists after subscription opens, clears mutation errors on change, and clears on disconnect", async () => {
    const queues: MessageQueue[] = [
      { revision: 1, maxMessages: 10, messages: [] },
      {
        revision: 2,
        maxMessages: 10,
        messages: [{ id: "q1", input: [{ type: "text", text: "from another client" }] }],
      },
    ];
    const list = vi.fn()
      .mockResolvedValueOnce(queues[0])
      .mockResolvedValueOnce(queues[1]);
    const deleteQueuedMessage = vi.fn().mockRejectedValue(new Error("busy"));
    let streamOptions: {
      onOpen?: () => void;
      onMessageQueueChanged?: (change: { sessionId: string; revision: number }) => void;
    } | undefined;
    let rejectStream: (error: Error) => void = () => undefined;
    const events = vi.fn((
      _sessionId: string,
      _afterEventId: undefined,
      _onEvent: () => void,
      _signal: AbortSignal,
      options: typeof streamOptions,
    ) => {
      streamOptions = options;
      return new Promise<void>((_resolve, reject) => { rejectStream = reject; });
    });
    setTalosClientFactory(() => ({
      messageQueue: { delete: deleteQueuedMessage, list },
      turns: { events },
    }) as unknown as TalosWebClient);

    const onQueueChanged = vi.fn();
    const { result } = renderHook(() =>
      useSessionMessageQueue("sess-1", true, onQueueChanged),
    );
    expect(list).not.toHaveBeenCalled();

    act(() => streamOptions?.onOpen?.());
    await waitFor(() => expect(result.current.queue?.revision).toBe(1));

    await act(async () => {
      await result.current.delete("q1").catch(() => undefined);
    });
    expect(result.current.error).toBe("busy");

    act(() => streamOptions?.onMessageQueueChanged?.({ sessionId: "sess-1", revision: 2 }));
    await waitFor(() => expect(result.current.queue?.revision).toBe(2));
    expect(result.current.error).toBeUndefined();
    expect(onQueueChanged).toHaveBeenCalledTimes(1);

    act(() => rejectStream(new Error("connection lost")));
    await waitFor(() => expect(result.current.queue).toBeUndefined());
    expect(list).toHaveBeenCalledTimes(2);
  });
});
