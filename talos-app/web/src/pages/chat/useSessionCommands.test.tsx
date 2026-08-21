// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import "@/shared/i18n/setup";
import type { ChatContextCompactionItem } from "@/features/chat";
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

  it("keeps observing until the matching backend compaction reaches a terminal outcome", async () => {
    const onCompactionObservationChange = vi.fn();
    let contextCompaction: ChatContextCompactionItem | undefined;
    const { result, rerender } = renderHook(() => useSessionCommands({
      contextCompaction,
      onCompactionObservationChange,
      sessionId: "sess-1",
    }));

    await act(async () => result.current.execute("compact"));

    expect(mocks.compact).toHaveBeenCalledTimes(1);
    expect(onCompactionObservationChange.mock.calls).toEqual([[true]]);
    expect(result.current.response).toEqual({
      kind: "compact",
      state: "running",
    });

    contextCompaction = compactionItem({ itemId: "compact-2", stage: "summarizing" });
    rerender();
    contextCompaction = compactionItem({
      completedAtMs: 2,
      itemId: "compact-2",
      outcome: "no_changes",
      stage: "completed",
      status: "completed",
    });
    rerender();

    expect(onCompactionObservationChange.mock.calls).toEqual([[true], [false]]);
    expect(result.current.response).toEqual({
      kind: "compact",
      outcome: "no_changes",
      state: "success",
    });
  });

  it("does not carry an in-flight manual compaction into another session", async () => {
    let resolveCompact: (() => void) | undefined;
    mocks.compact.mockReturnValue(new Promise<void>((resolve) => {
      resolveCompact = resolve;
    }));
    const onCompactionObservationChange = vi.fn();
    let sessionId = "sess-1";
    const { result, rerender } = renderHook(() => useSessionCommands({
      onCompactionObservationChange,
      sessionId,
    }));

    let execution: Promise<void> | void;
    act(() => {
      execution = result.current.execute("compact");
    });
    expect(result.current.response).toEqual({ kind: "compact", state: "running" });

    sessionId = "sess-2";
    rerender();

    expect(result.current.response).toBeUndefined();
    expect(onCompactionObservationChange.mock.calls).toEqual([[true], [false]]);

    await act(async () => {
      resolveCompact?.();
      await execution;
    });
    expect(result.current.response).toBeUndefined();

    sessionId = "sess-1";
    rerender();
    expect(result.current.response).toBeUndefined();
  });

  it("waits for a new backend item instead of reusing the previous compaction outcome", async () => {
    let contextCompaction = compactionItem({
      completedAtMs: 2,
      outcome: "applied",
      stage: "completed",
      status: "completed",
    });
    const { result, rerender } = renderHook(() => useSessionCommands({
      contextCompaction,
      sessionId: "sess-1",
    }));

    await act(async () => result.current.execute("compact"));
    expect(result.current.response).toEqual({ kind: "compact", state: "running" });

    contextCompaction = compactionItem({
      completedAtMs: 4,
      itemId: "compact-2",
      outcome: "no_changes",
      stage: "completed",
      startedAtMs: 3,
      status: "completed",
    });
    rerender();

    expect(result.current.response).toEqual({
      kind: "compact",
      outcome: "no_changes",
      state: "success",
    });
  });

  it("derives active compaction rendering from the backend stage", () => {
    const { result } = renderHook(() => useSessionCommands({
      contextCompaction: compactionItem({ stage: "summarizing" }),
      sessionId: "sess-1",
    }));

    expect(result.current.response).toEqual({
      kind: "compact",
      stage: "summarizing",
      state: "running",
    });
  });

  it("keeps automatic compaction out of the Composer response", () => {
    const { result } = renderHook(() => useSessionCommands({
      contextCompaction: compactionItem({ trigger: "threshold_exceeded" }),
      sessionId: "sess-1",
    }));

    expect(result.current.response).toBeUndefined();
  });

  it("settles a recovered manual compaction from its backend outcome", () => {
    let contextCompaction = compactionItem({ stage: "summarizing" });
    const { result, rerender } = renderHook(() => useSessionCommands({
      contextCompaction,
      sessionId: "sess-1",
    }));

    contextCompaction = compactionItem({
      completedAtMs: 2,
      outcome: "no_changes",
      stage: "completed",
      status: "running",
    });
    rerender();

    expect(result.current.response).toEqual({
      kind: "compact",
      outcome: "no_changes",
      state: "success",
    });
  });

  it("shows the busy notice when remote compaction wins the command race", async () => {
    mocks.compact.mockRejectedValue(new TalosApiError({
      code: AppProtocol.ErrorCode.ActiveTurnRunning,
      message: "Active turn is already running",
    }));
    const { result } = renderHook(() => useSessionCommands({
      sessionId: "sess-1",
    }));

    await act(async () => {
      await result.current.execute("compact")?.catch(() => undefined);
    });
    expect(result.current.response).toEqual({
      kind: "compact",
      message: "Context compacting... Please wait.",
      state: "error",
    });
  });

  it("requires confirmation before clearing the session", async () => {
    const { result } = renderHook(() => useSessionCommands({
      sessionId: "sess-1",
    }));

    act(() => result.current.execute("clear"));
    expect(result.current.clearConfirmationOpen).toBe(true);
    expect(mocks.clear).not.toHaveBeenCalled();

    await act(async () => result.current.confirmClear());
    expect(mocks.clear).toHaveBeenCalledTimes(1);
    expect(mocks.resetStream).toHaveBeenCalledWith("sess-1");
    expect(result.current.clearConfirmationOpen).toBe(false);
    expect(result.current.response).toEqual({ kind: "clear", state: "success" });
  });

  it("keeps the confirmation open and reports a clear failure", async () => {
    mocks.clear.mockRejectedValue(new Error("clear failed"));
    const { result } = renderHook(() => useSessionCommands({
      sessionId: "sess-1",
    }));

    act(() => result.current.execute("clear"));
    await act(async () => result.current.confirmClear());

    expect(result.current.clearConfirmationOpen).toBe(true);
    expect(result.current.clearError).toBe("Chat could not be cleared. Please try again.");
    expect(mocks.resetStream).not.toHaveBeenCalled();
  });

  it("opens and dismisses the structured session status", () => {
    const { result } = renderHook(() => useSessionCommands({
      sessionId: "sess-1",
    }));

    act(() => result.current.execute("status"));
    expect(result.current.response).toEqual({ kind: "status" });

    act(() => result.current.dismissResponse());
    expect(result.current.response).toBeUndefined();
  });

  it("reports a selected model through the same response surface", () => {
    const { result } = renderHook(() => useSessionCommands({
      sessionId: "sess-1",
    }));

    act(() => result.current.showModelChanged("GPT-5.6 Sol", "medium"));
    expect(result.current.response).toEqual({
      kind: "model",
      model: "GPT-5.6 Sol",
      reasoningLevel: "medium",
      state: "success",
    });
  });
});

function compactionItem(
  overrides: Partial<ChatContextCompactionItem> = {},
): ChatContextCompactionItem {
  return {
    itemId: "compact-1",
    kind: "context_compaction",
    phase: "system",
    status: "running",
    text: "Summarizing session context.",
    trigger: "user_requested",
    stage: "preparing",
    startedAtMs: 1,
    ...overrides,
  };
}
