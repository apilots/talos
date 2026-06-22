import { useEffect, useMemo, useState } from "react";
import { TalosHttpError } from "@talos/sdk-browser";

import { useSessionBlocks } from "./useSessionBlocks";
import { useSessionReconnect } from "./useSessionReconnect";
import { useSessionStreaming } from "./useSessionStreaming";
import {
  useActiveTurnQuery,
  useSessionHistoryQuery,
  useLiveRecoveryQuery,
} from "./useChatQueries";
import { createInitialTurnRequest } from "../api";
import { applyActiveTurnSnapshot, applyStreamEvent, clearInactiveTurnSnapshot } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";
import type { PendingInput, ProjectView, SessionInputHistory, SessionInputHistoryItem } from "@/shared/types/ui";
import type { SkillSuggestion } from "@talos/sdk-browser";

interface SendTextOptions {
  model?: string;
  project?: ProjectView;
  skills?: SkillSuggestion[];
}

export function useChatSession(
  sessionId: string,
  serverMessageCount?: number,
  sessionTitle?: string,
) {
  const [draft, setDraft] = useState("");
  const activeTurnQuery = useActiveTurnQuery(sessionId);
  const sessionHistoryQuery = useSessionHistoryQuery(sessionId);
  const shouldRecoverLiveTail = isLiveRecoverableActiveTurn(activeTurnQuery.data?.status);
  const liveRecoveryQuery = useLiveRecoveryQuery(
    sessionId,
    sessionHistoryQuery.data?.last_stable_event_id ?? undefined,
    shouldRecoverLiveTail && sessionHistoryQuery.isSuccess,
  );
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const pendingInput = streamState?.pendingInput;
  const pendingInputServerMessageId = streamState?.pendingInput?.serverMessageId;
  const {
    hasActiveConnection,
    isCancelling,
    isStreaming,
    reconnectMessage,
    retryResponse,
    sendMutation,
    sendText: streamText,
    stopResponse,
    streamState: liveStreamState,
    canRetryResponse,
  } =
    useSessionStreaming(sessionId, serverMessageCount, sessionTitle);
  const { retryStream } = useSessionReconnect({
    hasActiveConnection,
    reconnectMessage,
    sessionId,
    streamState: liveStreamState,
  });
  const blocks = useSessionBlocks({
    sessionId,
    streamState,
    sessionHistory: sessionHistoryQuery.data,
  });
  const inputHistory = useMemo(
    () => buildSessionInputHistory(sessionHistoryQuery.data, pendingInput),
    [pendingInput, sessionHistoryQuery.data],
  );

  useEffect(() => {
    if (!activeTurnQuery.data) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, activeTurnQuery.data);
  }, [activeTurnQuery.data, sessionId]);

  useEffect(() => {
    if (!liveRecoveryQuery.data?.active_turn) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, liveRecoveryQuery.data.active_turn);
  }, [sessionId, liveRecoveryQuery.data?.active_turn]);

  useEffect(() => {
    const events = liveRecoveryQuery.data?.replay.events ?? [];
    if (events.length === 0) {
      return;
    }
    [...events]
      .sort((left, right) => left.event_seq - right.event_seq)
      .forEach((event) => applyStreamEvent(sessionId, event));
  }, [sessionId, liveRecoveryQuery.data?.replay.events]);

  useEffect(() => {
    const error = activeTurnQuery.error;
    if (!(error instanceof TalosHttpError) || error.status !== 404) {
      return;
    }
    clearInactiveTurnSnapshot(sessionId);
  }, [activeTurnQuery.error, sessionId]);

  useEffect(() => {
    const activeMessageId = streamState?.activeMessageId;
    if (
      !activeMessageId ||
      streamState.isStreaming ||
      streamState.pendingUserAction
    ) {
      return;
    }

    const historyHasActiveMessage = Boolean(
      sessionHistoryQuery.data?.messages.some(
        (message) => message.message_id === activeMessageId,
      ),
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
      lastEventSeq: undefined,
      lastSequence: undefined,
      awaitingFinalResponse: false,
    });
  }, [
    sessionId,
    streamState?.activeMessageId,
    streamState?.isStreaming,
    streamState?.pendingUserAction,
    pendingInput,
    pendingInputServerMessageId,
    inputHistory,
    sessionHistoryQuery.data,
  ]);

  const sendText = (text: string, options?: SendTextOptions) => {
    const content = text.trim();
    if (!content) {
      return Promise.resolve();
    }
    const request = options
      ? createInitialTurnRequest(
          sessionId,
          useAppStore.getState().connection.userId,
          content,
          options.model,
          options.project,
          options.skills,
        )
      : undefined;
    return streamText(content, request);
  };

  return {
    blocks,
    inputHistory,
    draft,
    setDraft,
    sendText,
    sendTurn: (options?: SendTextOptions) => {
      const text = draft.trim();
      if (!text) {
        return Promise.resolve();
      }
      setDraft("");
      const request = options
        ? createInitialTurnRequest(
            sessionId,
            useAppStore.getState().connection.userId,
            text,
            options.model,
            options.project,
            options.skills,
          )
        : undefined;
      return sendMutation.mutateAsync({ text, appendUserInput: true, request });
    },
    retryStream,
    retryResponse,
    canRetryResponse,
    stopResponse,
    isStreaming,
    isCancelling,
    streamConnectionState: streamState?.connectionState ?? "idle",
    showReconnectControls: streamState?.showReconnectControls ?? false,
    streamError: streamState?.error,
    sessionHistoryQuery,
  };
}

export function buildSessionInputHistory(
  history: ReturnType<typeof useSessionHistoryQuery>["data"] | undefined,
  pendingInput: PendingInput | undefined,
): SessionInputHistory {
  const committed: SessionInputHistoryItem[] = [];

  let itemIndex = 0;
  (history?.messages ?? []).forEach((message) => {
    message.items.forEach((item) => {
      if (item.phase !== "input" || item.content.type !== "text") {
        return;
      }
      committed.push({
        messageId: message.message_id,
        itemIndex,
        text: item.content.text,
      });
      itemIndex += 1;
    });
  });

  return {
    committed,
    pendingInput,
  };
}

function isLiveRecoverableActiveTurn(status: string | undefined) {
  return status === "running" || status === "waiting_user_action" || status === "cancelling";
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
