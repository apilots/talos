// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import "@/shared/i18n/setup";
import { useSessionCommands } from "./useSessionCommands";

const mocks = vi.hoisted(() => ({
  clear: vi.fn(),
  compact: vi.fn(),
  resetStream: vi.fn(),
}));

vi.mock("@/features/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/features/chat")>();
  return {
    ...actual,
    useSessionActions: () => ({
      clear: { isPending: false, mutateAsync: mocks.clear },
      compact: { isPending: false, mutateAsync: mocks.compact },
    }),
  };
});

vi.mock("@/shared/store/app-store", () => ({
  useAppStore: { getState: () => ({ resetStream: mocks.resetStream }) },
}));

describe("useSessionCommands", () => {
  beforeEach(() => {
    mocks.clear.mockReset().mockResolvedValue(undefined);
    mocks.compact.mockReset().mockResolvedValue(undefined);
    mocks.resetStream.mockReset();
  });

  it("executes compact through the existing session action", async () => {
    const { result } = renderHook(() => useSessionCommands({
      currentModel: "model-1",
      isStreaming: false,
      sessionId: "sess-1",
    }));

    await act(async () => result.current.execute("compact"));

    expect(mocks.compact).toHaveBeenCalledTimes(1);
  });

  it("shows the busy notice when remote compaction wins the command race", async () => {
    mocks.compact.mockRejectedValue({ code: -32013 });
    const { result } = renderHook(() => useSessionCommands({
      currentModel: "model-1",
      isStreaming: false,
      sessionId: "sess-1",
    }));

    await act(async () => {
      await result.current.execute("compact")?.catch(() => undefined);
    });
    expect(result.current.notice).toBe("Context compacting... Please wait.");
  });
});
