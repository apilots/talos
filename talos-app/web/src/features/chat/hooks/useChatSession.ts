import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { apiErrorMessage } from "@/shared/i18n/apiErrorMessage";
import { AppProtocol, TalosApiError } from "@talos/sdk-browser";

import { useSessionBlocks } from "./useSessionBlocks";
import { useSessionStreaming } from "./useSessionStreaming";
import { useSessionMessageQueue } from "./useSessionMessageQueue";
import {
  useActiveTurnQuery,
  useSessionHistoryQuery,
} from "./useChatQueries";
import {
  isLiveRecoverableActiveTurn,
  useChatSessionTimelineLifecycle,
} from "./useChatSessionTimelineLifecycle";
import { createInitialTurnRequest, isActiveTurnRunningError } from "../api";
import {
  applyActiveTurnSnapshot,
  clearInactiveTurnSnapshot,
} from "../stream-events";
import { activeContextCompaction } from "../timeline-state";
import { buildQueuedInput, type SendTextOptions } from "../session-input";
export type { SendTextOptions } from "../session-input";
import { useProductScope } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { buildSessionInputHistory, sessionInputHistoryHasMessage } from "../session-input-history";

const ACTIVE_TURN_NOT_FOUND_CODES = [
  AppProtocol.ErrorCode.ResourceNotFound,
  AppProtocol.ErrorCode.SessionNotFound,
] as const satisfies readonly AppProtocol.ErrorCode[];

export function useChatSession(
  sessionId: string,
  serverMessageCount?: number,
) {
  useTranslation("errors");
  const scope = useProductScope();
  const draftKey = JSON.stringify([scope, sessionId]);
  const draft = useAppStore((state) => state.composerDrafts[draftKey] ?? "");
  const saveDraft = useAppStore((state) => state.saveComposerDraft);
  const setDraft = (text: string) => saveDraft(draftKey, text);
  const sessionAvailable = serverMessageCount !== undefined;
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const sessionHistoryQuery = useSessionHistoryQuery(sessionId, sessionAvailable);
  const refetchHistory = sessionHistoryQuery.refetch;
  const activeTurnQuery = useActiveTurnQuery(
    sessionId,
    sessionHistoryQuery.isSuccess && sessionHistoryQuery.isFetchedAfterMount && !streamState?.isStreaming,
  );
  const activeTurn = activeTurnQuery.data?.activeTurn;
  const messageQueue = useSessionMessageQueue(sessionId, sessionAvailable && !streamState?.isStreaming);
  const {
    contextCompaction,
    retryTimeline,
    setCompactionObservationActive,
  } = useChatSessionTimelineLifecycle({
    activeTurnQuery,
    messageQueue,
    sessionHistoryQuery,
    sessionId,
    sessionAvailable,
    streamState,
  });
  const pendingInput = streamState?.pendingInput;
  const pendingInputServerMessageId = streamState?.pendingInput?.serverMessageId;
  const {
    isCancelling,
    isStreaming,
    retryResponse,
    sendMutation,
    sendText: streamText,
    stopResponse,
    canRetryResponse,
  } =
    useSessionStreaming(sessionId);
  const blocks = useSessionBlocks({
    sessionId,
    streamState,
    sessionHistory: sessionHistoryQuery.data,
  });
  const inputHistory = useMemo(
    () => buildSessionInputHistory(sessionHistoryQuery.data, pendingInput),
    [pendingInput, sessionHistoryQuery.data],
  );
  const activeTurnId = streamState?.isStreaming
    ? streamState.activeTurnId
    : isLiveRecoverableActiveTurn(activeTurn?.state)
      ? activeTurn?.identity.turnId
      : undefined;

  useEffect(() => {
    if (!activeTurn || !activeTurnQuery.isFetchedAfterMount || !sessionHistoryQuery.isSuccess) {
      return;
    }
    // Once replay has established a Turn, only ordered events may advance it.
    if (sessionHistoryQuery.data?.data.some((turn) =>
      turn.identity.turnId === activeTurn?.identity.turnId,
    ) || useAppStore.getState().streams[sessionId]?.activeTurnId === activeTurn.identity.turnId) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, activeTurn);
  }, [activeTurn, activeTurnQuery.isFetchedAfterMount, sessionHistoryQuery.data, sessionHistoryQuery.isSuccess, sessionId]);

  useEffect(() => {
    const error = activeTurnQuery.error;
    if (activeTurn === null && !useAppStore.getState().streams[sessionId]?.isStreaming) {
      clearInactiveTurnSnapshot(sessionId);
      // The snapshot cursor also changes for remote Turns that finish between polls.
      if (activeTurnQuery.isFetchedAfterMount) {
        void refetchHistory();
      }
      return;
    }
    if (
      !(error instanceof TalosApiError) ||
      !ACTIVE_TURN_NOT_FOUND_CODES.some((code) => code === error.code)
    ) {
      return;
    }
    clearInactiveTurnSnapshot(sessionId);
  }, [activeTurnQuery.data, activeTurnQuery.error, activeTurnQuery.isFetchedAfterMount, refetchHistory, sessionId]);

  useEffect(() => {
    const activeMessageId = streamState?.activeMessageId;
    if (!activeMessageId) {
      return;
    }

    const historyHasActiveMessage = Boolean(
      sessionHistoryQuery.data?.data.some((turn) =>
        turn.result.result.messages.some((message) => message.messageId === activeMessageId),
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
      activeInput: undefined,
      activeClientMessageId: undefined,
      lastSequence: undefined,
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
    const fileIds = options?.fileIds ?? [];
    if (!content && fileIds.length === 0) {
      return Promise.resolve();
    }
    if (activeContextCompaction(useAppStore.getState().streams[sessionId])) {
      return Promise.resolve();
    }
    if (activeTurnId) {
      return messageQueue.add(buildQueuedInput(content, options)).then(() => undefined);
    }
    const request = options
      ? createInitialTurnRequest(
          sessionId,
          content,
          {
            ...(options.nodeId ? { nodeId: options.nodeId } : {}),
            fileIds,
            modelSelection: options.modelSelection,
            project: options.project,
            userPermissionMode: options.userPermissionMode,
          },
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
      const fileIds = options?.fileIds ?? [];
      if (!text && fileIds.length === 0) {
        return Promise.resolve();
      }
      if (activeContextCompaction(useAppStore.getState().streams[sessionId])) {
        return Promise.resolve();
      }
      if (activeTurnId) {
        return messageQueue.add(buildQueuedInput(text, options)).then(() => {
          setDraft("");
        });
      }
      setDraft("");
      const request = options
        ? createInitialTurnRequest(
            sessionId,
            text,
            {
              ...(options.nodeId ? { nodeId: options.nodeId } : {}),
            fileIds,
              modelSelection: options.modelSelection,
              project: options.project,
              userPermissionMode: options.userPermissionMode,
            },
          )
        : undefined;
      return sendMutation.mutateAsync({ text, request }).catch((error: unknown) => {
        if (isActiveTurnRunningError(error)) {
          setDraft(text);
          return;
        }
        throw error;
      });
    },
    retryStream: retryTimeline,
    retryResponse,
    canRetryResponse,
    stopResponse: () => activeTurnId ? stopResponse(activeTurnId) : Promise.resolve(),
    isStreaming,
    isCancelling,
    contextCompaction,
    setCompactionObservationActive,
    activeTurnId,
    messageQueue: {
      ...messageQueue,
      activeTurnId,
      start: async (messageId?: string) => {
        const response = await messageQueue.start(messageId);
        await activeTurnQuery.refetch();
        return response;
      },
    },
    pendingUserAction: streamState?.pendingUserAction,
    streamConnectionState: streamState?.connectionState ?? "idle",
    showReconnectControls: streamState?.showReconnectControls ?? false,
    streamError: streamState?.error ? apiErrorMessage(streamState.error) : undefined,
    sessionHistoryQuery,
  };
}
