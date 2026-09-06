import { useEffect, useMemo, useState } from "react";
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
import { useAppStore } from "@/shared/store/app-store";
import type {
  PendingInput,
  SessionInputHistory,
  SessionInputHistoryItem,
} from "@/shared/types/ui";

const ACTIVE_TURN_NOT_FOUND_CODES = [
  AppProtocol.ErrorCode.ResourceNotFound,
  AppProtocol.ErrorCode.SessionNotFound,
] as const satisfies readonly AppProtocol.ErrorCode[];

export function useChatSession(
  sessionId: string,
  serverMessageCount?: number,
) {
  const [draft, setDraft] = useState("");
  const sessionAvailable = serverMessageCount !== undefined;
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const activeTurnQuery = useActiveTurnQuery(
    sessionId,
    sessionAvailable && !streamState?.isStreaming,
  );
  const sessionHistoryQuery = useSessionHistoryQuery(sessionId, sessionAvailable);
  const messageQueue = useSessionMessageQueue(sessionId);
  const {
    contextCompaction,
    retryTimeline,
    setCompactionObservationActive,
  } = useChatSessionTimelineLifecycle({
    messageQueue,
    sessionHistoryQuery,
    sessionId,
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
    : isLiveRecoverableActiveTurn(activeTurnQuery.data?.state)
      ? activeTurnQuery.data?.identity.turnId
      : undefined;

  useEffect(() => {
    if (!activeTurnQuery.data) {
      return;
    }
    // Once replay has established a Turn, only ordered events may advance it.
    if (useAppStore.getState().streams[sessionId]?.activeTurnId) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, activeTurnQuery.data);
  }, [activeTurnQuery.data, sessionId]);

  useEffect(() => {
    const error = activeTurnQuery.error;
    if (activeTurnQuery.data === null && !useAppStore.getState().streams[sessionId]?.isStreaming) {
      clearInactiveTurnSnapshot(sessionId);
      return;
    }
    if (
      !(error instanceof TalosApiError) ||
      !ACTIVE_TURN_NOT_FOUND_CODES.some((code) => code === error.code)
    ) {
      return;
    }
    clearInactiveTurnSnapshot(sessionId);
  }, [activeTurnQuery.data, activeTurnQuery.error, sessionId]);

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
      lastEventSeq: undefined,
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
    },
    pendingUserAction: streamState?.pendingUserAction,
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
  (history?.data ?? []).forEach((turn) => {
    turn.result.result.messages.forEach((message) => {
      if (message.role !== "user") {
        return;
      }
      const text: string[] = [];
      const fileIds: string[] = [];
      message.blocks.forEach((block) => {
        if (block.content.type !== "userContent") {
          return;
        }
        if (block.content.data.type === "text") {
          text.push(block.content.data.text);
        } else if (block.content.data.type === "file") {
          fileIds.push(block.content.data.fileId);
        }
      });
      if (text.length > 0 || fileIds.length > 0) {
        committed.push({
          messageId: message.messageId,
          itemIndex,
          text: text.join("\n"),
          fileIds,
        });
        itemIndex += 1;
      }
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
