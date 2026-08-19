import { describe, expect, it, vi } from "vitest";
import type { MessageQueue } from "@talos/sdk-browser";

import { MessageQueueSync } from "./message-queue-sync";

const queue = (revision: number): MessageQueue => ({ revision, maxMessages: 10, messages: [] });

describe("MessageQueueSync", () => {
  it("coalesces changes during one list and converges to the highest revision", async () => {
    let resolveFirst: (value: ReturnType<typeof queue>) => void = () => undefined;
    const list = vi
      .fn()
      .mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce(queue(3));
    const onChange = vi.fn();
    const sync = new MessageQueueSync({ list, onChange });

    sync.observe(1);
    sync.observe(2);
    sync.observe(3);
    expect(list).toHaveBeenCalledTimes(1);
    resolveFirst(queue(1));
    await sync.refresh();

    expect(list).toHaveBeenCalledTimes(2);
    expect(onChange).toHaveBeenLastCalledWith(queue(3));
  });

  it("ignores stale mutation responses and clears process-local state on disconnect", () => {
    const onChange = vi.fn();
    const sync = new MessageQueueSync({ list: vi.fn(), onChange });

    sync.apply(queue(4));
    sync.apply(queue(2));
    sync.clear();

    expect(onChange.mock.calls).toEqual([[queue(4)], [undefined]]);
  });

  it("does not apply a list response from a cleared connection", async () => {
    let resolveList: (value: ReturnType<typeof queue>) => void = () => undefined;
    const changes: Array<ReturnType<typeof queue> | undefined> = [];
    const sync = new MessageQueueSync({
      list: () => new Promise((resolve) => { resolveList = resolve; }),
      onChange: (value) => changes.push(value),
    });

    const refresh = sync.refresh();
    sync.clear();
    resolveList(queue(1));
    await refresh;

    expect(changes).toEqual([undefined]);
  });
});
