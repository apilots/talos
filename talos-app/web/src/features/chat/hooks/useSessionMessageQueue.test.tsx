// @vitest-environment jsdom

import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TalosApiError, type MessageQueue } from "@talos/sdk-browser";
import { i18n } from "@/shared/i18n/setup";

import { useSessionMessageQueue } from "./useSessionMessageQueue";
import {
  resetTalosClientFactory,
  setTalosClientFactory,
  type TalosWebClient,
} from "@/shared/api/client";

afterEach(async () => { resetTalosClientFactory(); await i18n.changeLanguage("en"); });

describe("useSessionMessageQueue", () => {
  it("retranslates a queued operation failure without retrying the operation", async () => {
    const add = vi.fn().mockRejectedValue(new TalosApiError({
      code: "attachmentLimitExceeded", message: "Too many attachments: 6 supplied, 4 allowed.",
      localization: { type: "attachmentCountExceeded", limit: 4, actual: 6 },
    }));
    setTalosClientFactory(() => ({ messageQueue: { add } }) as unknown as TalosWebClient);
    const { result } = renderHook(() => useSessionMessageQueue("sess-1"));
    await act(async () => { await result.current.add({ content: [] }).catch(() => undefined); });
    expect(result.current.error).toBe("This request contains 6 attachments; the limit is 4.");
    await act(() => i18n.changeLanguage("zh-CN"));
    expect(result.current.error).toMatchInlineSnapshot('"本次请求包含 6 个附件，上限为 4 个。"');
    expect(add).toHaveBeenCalledTimes(1);
  });
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
