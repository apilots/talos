// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

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
      sessionId: "sess-1",
    }));

    await act(async () => result.current.execute("compact"));

    expect(mocks.compact).toHaveBeenCalledTimes(1);
    expect(result.current.response).toEqual({
      kind: "compact",
      state: "success",
    });
  });

  it("derives active compaction rendering from the backend stage", () => {
    const { result } = renderHook(() => useSessionCommands({
      contextCompactionStage: "summarizing",
      sessionId: "sess-1",
    }));

    expect(result.current.response).toEqual({
      kind: "compact",
      stage: "summarizing",
      state: "running",
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
