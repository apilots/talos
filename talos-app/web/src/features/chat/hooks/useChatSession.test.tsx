// @vitest-environment jsdom

import { getChatSessionMocks, resetChatSessionMocks } from "./useChatSession.test-support";
import { applyStreamEvent } from "../stream-events";
import { contextCompactionStarted } from "../__tests__/streaming-fixtures";

import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { AppProtocol } from "@talos/sdk-browser";

import { useChatSession } from "./useChatSession";
import { useAppStore } from "@/shared/store/app-store";


const mocks = getChatSessionMocks();

describe("useChatSession", () => {
  beforeEach(resetChatSessionMocks);

  it("starts the running Turn query while History is still loading and waits to subscribe", () => {
    mocks.historyLoaded = false;
    const { rerender } = renderHook(() => useChatSession("session-1", 1));
    expect(mocks.activeTurnQuery).toHaveBeenLastCalledWith("session-1", true);
    expect(mocks.timeline.mock.lastCall?.[2]).toBe(false);
    mocks.historyLoaded = true;
    rerender();
    expect(mocks.timeline.mock.lastCall?.[2]).toBe(true);
  });

  it("shows and retries a failed running Turn query without replaying the backlog", async () => {
    mocks.activeTurnError = new Error("Running Turn unavailable");
    const { result, rerender } = renderHook(() => useChatSession("session-1", 1));
    expect(mocks.timeline.mock.lastCall?.[2]).toBe(false);
    expect(result.current.showReconnectControls).toBe(true);
    expect(result.current.streamError).toBe("Running Turn unavailable");
    await act(async () => result.current.retryStream());
    expect(mocks.activeTurnRefetch).toHaveBeenCalledOnce();
    mocks.activeTurnError = null;
    mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    rerender();
    expect(mocks.timeline.mock.lastCall?.[2]).toBe(true);
    expect(result.current.streamError).toBeUndefined();
  });

  it("restores the draft when the server reports an active turn", async () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));
    act(() => result.current.setDraft("keep this input"));

    await act(async () => { await expect(result.current.sendTurn()).rejects.toThrow("Active turn is already running"); });

    expect(mocks.send).toHaveBeenCalledTimes(1);
    expect(result.current.draft).toBe("keep this input");
  });

  it("starts the new session timeline after the server accepts the local turn", () => {
    const { rerender } = renderHook(() => useChatSession("session-1"));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
      expect.objectContaining({ stableEventSeq: 0, reloadHistory: expect.any(Function) }),
    );

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", {
        isStreaming: true,
      });
    });
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      false,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
      expect.objectContaining({ stableEventSeq: 0, reloadHistory: expect.any(Function) }),
    );

    act(() => {
      useAppStore.getState().setStreamMeta("session-1", {
        activeMessageId: "message-1",
        isStreaming: true,
      });
    });
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      true,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
      expect.objectContaining({ stableEventSeq: 0, reloadHistory: expect.any(Function) }),
    );
  });

  it("observes idle-session maintenance before a manual command starts", () => {
    const { result } = renderHook(() => useChatSession("session-1", 0));

    act(() => result.current.setCompactionObservationActive(true));

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-1",
      undefined,
      true,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
      expect.objectContaining({ stableEventSeq: 0, reloadHistory: expect.any(Function) }),
    );
  });

  it("does not transfer idle maintenance observation to another session", () => {
    let sessionId = "session-1";
    const { result, rerender } = renderHook(() => useChatSession(sessionId));

    act(() => result.current.setCompactionObservationActive(true));
    sessionId = "session-2";
    rerender();

    expect(mocks.timeline).toHaveBeenLastCalledWith(
      "session-2",
      undefined,
      false,
      expect.any(Function),
      expect.objectContaining({ start: mocks.messageQueueStart }),
      expect.objectContaining({ stableEventSeq: 0, reloadHistory: expect.any(Function) }),
    );
  });

  it("does not poll active turn snapshots while a local event stream is active", () => {
    mocks.activeTurn = {
      identity: { sessionId: "session-1", turnId: "turn-1" },
      initialMessageId: "message-1",
      state: "waitingForInput",
      lastEventId: "event-1",
      updatedAt: 1,
    };
    useAppStore.getState().setStreamMeta("session-1", {
      activeMessageId: "message-1",
      isStreaming: true,
    });

    renderHook(() => useChatSession("session-1", 1));

    expect(mocks.activeTurnQuery).toHaveBeenLastCalledWith("session-1", false);
  });

  it("refetches authoritative state when the timeline reports a terminal turn", async () => {
    renderHook(() => useChatSession("session-1", 1));
    const onTerminalTurn = mocks.timeline.mock.lastCall?.[3] as
      | (() => Promise<void>)
      | undefined;

    expect(onTerminalTurn).toBeDefined();
    await act(async () => onTerminalTurn?.());

    expect(mocks.sessionHistoryRefetch).toHaveBeenCalledTimes(1);
    expect(mocks.activeTurnRefetch).toHaveBeenCalledTimes(1);
  });

  it("allows an attachment-only turn and forwards ordered file ids", async () => {
    mocks.send.mockResolvedValueOnce(undefined);
    const { result } = renderHook(() => useChatSession("session-1", 0));

    await act(() => result.current.sendTurn({ fileIds: ["file-2", "file-1"] }));

    expect(mocks.createInitialTurnRequest).toHaveBeenCalledWith(
      "session-1",
      "",
      {
        fileIds: ["file-2", "file-1"],
        modelSelection: undefined,
        project: undefined,
        userPermissionMode: undefined,
      },
    );
    expect(mocks.send).toHaveBeenCalledTimes(1);
  });

  it("queues during the accepted-start window before the first Timeline Turn identity arrives", async () => {
    mocks.isStreaming = true;
    mocks.messageQueueAdd.mockClear().mockResolvedValue(undefined);
    const { result } = renderHook(() => useChatSession("session-1", 1));
    act(() => result.current.setDraft("Next input"));
    await act(async () => result.current.sendTurn());
    expect(mocks.messageQueueAdd).toHaveBeenCalledWith({ content: [{ type: "text", text: "Next input" }] });
    expect(mocks.send).not.toHaveBeenCalled();
    expect(result.current.messageQueue.canStart).toBe(false);
  });

  it("queues when the policy is unavailable and steers when it becomes available", async () => {
    mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    const { result, rerender } = renderHook(
      ({ policy }: { policy?: AppProtocol.SessionMessageSendPolicy }) =>
        useChatSession("session-1", 1, undefined, policy),
      { initialProps: {} },
    );
    act(() => result.current.setDraft("Wait for the next turn"));
    await act(() => result.current.sendTurn());
    expect(mocks.messageQueueAdd).toHaveBeenCalledWith({
      content: [{ type: "text", text: "Wait for the next turn" }],
    });

    rerender({ policy: "steer" });
    act(() => result.current.setDraft("Use this now"));
    await act(() => result.current.sendTurn({ fileIds: ["file-1"] }));
    expect(mocks.submitTurnInput).toHaveBeenCalledWith({
      identity: { sessionId: "session-1", turnId: "turn-1" },
      input: { type: "message", input: { content: [{ type: "text", text: "Use this now" }, { type: "file", fileId: "file-1" }] } },
      clientRequestId: null,
    });
    expect(mocks.messageQueueAdd).toHaveBeenCalledTimes(1);
    expect(result.current.draft).toBe("");
  });

  it("keeps the draft when the targeted Turn finishes before Steer is accepted", async () => {
    mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    const failure = { code: AppProtocol.ErrorCode.ResourceNotFound };
    mocks.submitTurnInput.mockRejectedValue(failure);
    const { result } = renderHook(() => useChatSession("session-1", 1, undefined, "steer"));
    act(() => result.current.setDraft("Keep this follow-up"));
    await expect(result.current.sendTurn()).rejects.toBe(failure);
    expect(result.current.draft).toBe("Keep this follow-up");
    expect(mocks.messageQueueAdd).not.toHaveBeenCalled();
  });

  it("rejects sendText during automatic context compaction instead of acknowledging a discarded input", async () => {
    const event = contextCompactionStarted("compact-1", "summarizing", null, 1);
    event.event.data.identity.sessionId = "session-1";
    applyStreamEvent("session-1", event);
    mocks.messageQueueAdd.mockClear();
    const { result } = renderHook(() => useChatSession("session-1", 1));
    await expect(result.current.sendText("Keep this input")).rejects.toMatchObject({ code: AppProtocol.ErrorCode.Conflict });
    expect(mocks.messageQueueAdd).not.toHaveBeenCalled();
    expect(mocks.send).not.toHaveBeenCalled();
  });



});
