// @vitest-environment jsdom

import { getChatSessionMocks, resetChatSessionMocks } from "./useChatSession.test-support";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";
import { useChatSession } from "./useChatSession";

const mocks = getChatSessionMocks();

describe("useChatSession automation admission", () => {
  beforeEach(resetChatSessionMocks);

  it.each([undefined, { name: "loop", path: "/worker/system/loop/SKILL.md" }])("keeps running-session messages in the original queue (%j)", async (skillSelection) => {
    mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    mocks.messageQueueAdd.mockClear().mockResolvedValue(undefined);
    const { result } = renderHook(() => useChatSession("session-1", 1));
    act(() => result.current.setDraft("Keep human input"));
    await act(async () => result.current.sendTurn({ skillSelection }));
    expect(mocks.send).not.toHaveBeenCalled();
    expect(mocks.messageQueueAdd).toHaveBeenCalledWith({ content: [
      ...(skillSelection ? [{ type: "skillSelection", ...skillSelection }] : []),
      { type: "text", text: "Keep human input" },
    ] });
    expect(result.current.draft).toBe("");
  });

  it("queues a Talo slash activation for Server normalization even with steering enabled", async () => {
    mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    const { result } = renderHook(() => useChatSession("session-1", 1, "ready", "steer"));
    act(() => result.current.setDraft("/goal Fix CI"));
    await act(async () => result.current.sendTurn({ skillSelection: { name: "goal", path: "" } }));
    expect(mocks.messageQueueAdd).toHaveBeenCalledWith({ content: [
      { type: "skillSelection", name: "goal", path: "" },
      { type: "text", text: "/goal Fix CI" },
    ] });
    expect(mocks.submitTurnInput).not.toHaveBeenCalled();
  });

  it.each([false, true])("sends the shared slash prompt through original admission/queue while preserving raw draft on failure (busy=%s)", async (busy) => {
    if (!busy) {
      const actual = await vi.importActual<typeof import("../api")>("../api");
      mocks.createInitialTurnRequest.mockImplementationOnce(actual.createInitialTurnRequest).mockImplementationOnce(actual.createInitialTurnRequest);
    }
    if (busy) mocks.activeTurn = { state: "running", identity: { sessionId: "session-1", turnId: "turn-1" } };
    const raw = "/loop 5m  Check deployment";
    const prompt = "5m  Check deployment";
    const skillSelection = { name: "loop", path: "/worker/system/loop/SKILL.md" };
    const failure = new TalosApiError({ code: AppProtocol.ErrorCode.Conflict, message: "Admission rejected", localization: null });
    const submit = busy ? mocks.messageQueueAdd : mocks.send;
    submit.mockReset().mockRejectedValueOnce(failure).mockResolvedValue(undefined);
    const { result } = renderHook(() => useChatSession("session-1", 1));
    act(() => result.current.setDraft(raw));
    await act(async () => { await expect(result.current.sendTurn({ skillSelection }, prompt)).rejects.toBe(failure); });
    expect(result.current.draft).toBe(raw);
    if (busy) expect(submit).toHaveBeenLastCalledWith({ content: [{ type: "skillSelection", ...skillSelection }, { type: "text", text: prompt }] });
    else expect(submit).toHaveBeenLastCalledWith({ text: prompt, request: expect.objectContaining({ input: { content: [{ type: "skillSelection", ...skillSelection }, { type: "text", text: prompt }] } }) as unknown });
    await act(async () => result.current.sendTurn({ skillSelection }, prompt));
    expect(result.current.draft).toBe("");
    if (!busy) expect(mocks.createInitialTurnRequest).toHaveBeenLastCalledWith("session-1", prompt, expect.objectContaining({ skillSelection }));
  });
});
