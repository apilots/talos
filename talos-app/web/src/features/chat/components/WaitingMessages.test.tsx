// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { WaitingMessages } from "./WaitingMessages";

afterEach(cleanup);

const queue = {
  revision: 2,
  maxMessages: 10,
  messages: [
    {
      id: "q1",
      input: {
        content: [
          { type: "text" as const, text: "First waiting message" },
          { type: "file" as const, fileId: "file-1" },
        ],
      },
    },
    {
      id: "q2",
      input: {
        content: [{ type: "skillSelection" as const, name: "review", path: "/skills/review" }],
      },
    },
  ],
};

const steerQueue = {
  ...queue,
  messages: [
    queue.messages[0]!,
    {
      id: "q3",
      input: { content: [{ type: "text" as const, text: "Second steer message" }] },
    },
  ],
};

function handlers() {
  return {
    delete: vi.fn().mockResolvedValue(undefined),
    reorder: vi.fn().mockResolvedValue(undefined),
    start: vi.fn().mockResolvedValue(undefined),
    steer: vi.fn().mockResolvedValue(undefined),
    update: vi.fn().mockResolvedValue(undefined),
  };
}

describe("WaitingMessages", () => {
  it("renders one continuous queue region and only offers Steer for supported input", () => {
    const actions = handlers();
    render(<WaitingMessages controller={{ activeTurnId: "turn-1", queue, ...actions }} />);

    const region = screen.getByTestId("waiting-messages");
    expect(region.className).toContain("border-b");
    expect(screen.getAllByRole("button", { name: "Steer" })).toHaveLength(1);
    fireEvent.click(screen.getAllByRole("button", { name: "Delete queued message" })[0]!);
    expect(actions.delete).toHaveBeenCalledWith("q1");
  });

  it("edits text without dropping attachments and reorders by complete id list", async () => {
    const actions = handlers();
    render(<WaitingMessages controller={{ queue, ...actions }} />);

    fireEvent.click(screen.getAllByRole("button", { name: "More message actions" })[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Edit message" }));
    const input = screen.getByDisplayValue("First waiting message");
    fireEvent.change(input, { target: { value: "Updated message" } });
    fireEvent.keyDown(input, { key: "Enter", code: "Enter" });
    await waitFor(() => {
      expect(actions.update).toHaveBeenCalledWith("q1", {
        content: [
          { type: "text", text: "Updated message" },
          { type: "file", fileId: "file-1" },
        ],
      });
    });

    fireEvent.click(screen.getAllByRole("button", { name: "More message actions" })[0]!);
    fireEvent.click(screen.getByRole("button", { name: "Move down" }));
    expect(actions.reorder).toHaveBeenCalledWith(["q2", "q1"]);
  });

  it("submits each pending Steer once and restores its actions after completion", async () => {
    const first = deferred<void>();
    const second = deferred<void>();
    const actions = handlers();
    actions.steer.mockImplementation((messageId) =>
      messageId === "q1" ? first.promise : second.promise,
    );
    render(
      <WaitingMessages controller={{ activeTurnId: "turn-1", queue: steerQueue, ...actions }} />,
    );

    const steerButtons = screen.getAllByRole("button", { name: "Steer" });
    fireEvent.click(steerButtons[0]!);
    fireEvent.click(steerButtons[0]!);
    fireEvent.click(steerButtons[1]!);

    expect(actions.steer.mock.calls).toEqual([
      ["q1", "turn-1"],
      ["q3", "turn-1"],
    ]);
    expect((steerButtons[0] as HTMLButtonElement).disabled).toBe(true);
    expect((steerButtons[1] as HTMLButtonElement).disabled).toBe(true);
    expect(
      (screen.getAllByRole("button", { name: "Delete queued message" })[0] as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(
      (screen.getAllByRole("button", { name: "More message actions" })[0] as HTMLButtonElement)
        .disabled,
    ).toBe(false);
    expect(
      screen.getByTitle("First waiting message · 1 attachment").closest(".group")?.className,
    ).not.toContain("hover:bg-muted/5");

    first.resolve();
    await waitFor(() => expect((steerButtons[0] as HTMLButtonElement).disabled).toBe(false));
    expect((steerButtons[1] as HTMLButtonElement).disabled).toBe(true);
    expect(
      screen.getByTitle("First waiting message · 1 attachment").closest(".group")?.className,
    ).toContain("hover:bg-muted/5");

    second.resolve();
    await waitFor(() => expect((steerButtons[1] as HTMLButtonElement).disabled).toBe(false));
  });
});

function deferred<T>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  const promise = new Promise<T>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return { promise, resolve };
}
