import { useEffect, useMemo, useState } from "react";

import { useSessionApproval } from "./useSessionApproval";
import { useSessionBlocks } from "./useSessionBlocks";
import { useSessionReconnect } from "./useSessionReconnect";
import { useSessionStreaming } from "./useSessionStreaming";
import { useActiveTurnQuery, useSessionTurnsQuery } from "./useChatQueries";
import { applyActiveTurnSnapshot } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import type { ChatApiTurns } from "../api";
import type { PendingInput, SessionInputHistory, SessionInputHistoryItem } from "@/shared/types/ui";

export function useChatSession(
  sessionId: string,
  serverMessageCount?: number,
  sessionTitle?: string,
) {
  const [draft, setDraft] = useState("");
  const activeTurnQuery = useActiveTurnQuery(sessionId);
  const turnsQuery = useSessionTurnsQuery(sessionId);
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const pendingInput = streamState?.pendingInput;
  const pendingInputServerMessageId = streamState?.pendingInput?.serverMessageId;
  const {
    continueAfterApproval,
    hasActiveConnection,
    isCancelling,
    isStreaming,
    reconnectMessage,
    retryResponse,
    sendMutation,
    sendText,
    stopResponse,
    streamState: liveStreamState,
    canRetryResponse,
  } =
    useSessionStreaming(sessionId, serverMessageCount, sessionTitle);
  const { pendingApproval, submitApproval } = useSessionApproval(sessionId, continueAfterApproval);
  const { retryStream } = useSessionReconnect({
    hasActiveConnection,
    reconnectMessage,
    serverMessageCount,
    sessionId,
    streamState: liveStreamState,
  });
  const blocks = useSessionBlocks({
    sessionId,
    streamState,
    turns: turnsQuery.data,
  });
  const inputHistory = useMemo(
    () => buildSessionInputHistory(turnsQuery.data, pendingInput),
    [pendingInput, turnsQuery.data],
  );

  useEffect(() => {
    if (!activeTurnQuery.data) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, activeTurnQuery.data);
  }, [activeTurnQuery.data, sessionId]);

  useEffect(() => {
    const activeMessageId = streamState?.activeMessageId;
    if (
      !activeMessageId ||
      streamState.isStreaming ||
      streamState.pendingApproval ||
      streamState.pendingUserAction
    ) {
      return;
    }

    const historyHasActiveMessage = (turnsQuery.data ?? []).some(
      (turn) => turn.message_id === activeMessageId,
    );
    if (!historyHasActiveMessage) {
      return;
    }

    const historyHasPendingInput =
      Boolean(pendingInputServerMessageId) &&
      sessionInputHistoryHasMessage(inputHistory, pendingInputServerMessageId);
    const state = useAppStore.getState();
    state.resetLiveResponse(sessionId);
    if (historyHasPendingInput) {
      state.setPendingInput(sessionId, undefined);
    }
    state.setStreamMeta(sessionId, {
      activeMessageId: undefined,
      activeInputText: undefined,
      activeClientMessageId: undefined,
      lastSequence: undefined,
      expectedMessageCount: undefined,
      awaitingFinalResponse: false,
    });
  }, [
    sessionId,
    streamState?.activeMessageId,
    streamState?.isStreaming,
    streamState?.pendingApproval,
    streamState?.pendingUserAction,
    pendingInput,
    pendingInputServerMessageId,
    inputHistory,
    turnsQuery.data,
  ]);

  return {
    blocks,
    inputHistory,
    draft,
    setDraft,
    sendText,
    sendMessage: () => {
      const text = draft.trim();
      if (!text) {
        return Promise.resolve();
      }
      setDraft("");
      return sendMutation.mutateAsync({ text, appendUserInput: true });
    },
    retryStream,
    retryResponse,
    canRetryResponse,
    stopResponse,
    submitApproval,
    isStreaming,
    isCancelling,
    streamConnectionState: streamState?.connectionState ?? "idle",
    showReconnectControls: streamState?.showReconnectControls ?? false,
    pendingApproval,
    streamError: streamState?.error,
    turnsQuery,
  };
}

export function buildSessionInputHistory(
  turns: ChatApiTurns | undefined,
  pendingInput: PendingInput | undefined,
): SessionInputHistory {
  const committed: SessionInputHistoryItem[] = [];

  (turns ?? []).forEach((turn) => {
    turn.output.forEach((item, itemIndex) => {
      if (item.type !== "user_input") {
        return;
      }
      committed.push({
        messageId: turn.message_id,
        itemIndex,
        text: item.text,
      });
    });
  });

  return {
    committed,
    pendingInput,
  };
}

function sessionInputHistoryHasMessage(
  inputHistory: SessionInputHistory,
  messageId: string | undefined,
) {
  if (!messageId) {
    return false;
  }
  return inputHistory.committed.some((item) => item.messageId === messageId);
}
