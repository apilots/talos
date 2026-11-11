import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { apiErrorMessage, userVisibleErrorMessage } from "@/shared/i18n/apiErrorMessage";
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
import { createInitialTurnRequest, submitTurnInput } from "../api";
import {
  clearInactiveTurnSnapshot,
} from "../stream-events";
import { activeContextCompaction } from "../timeline-state";
import { buildQueuedInput, type SendTextOptions } from "../session-input";
export type { SendTextOptions } from "../session-input";
import { useProductScope } from "@/features/computers";
import { useAppStore } from "@/shared/store/app-store";
import { buildSessionInputHistory, sessionInputHistoryHasMessage } from "../session-input-history";
import { automationTurnTiming } from "../automation-runtime";

const ACTIVE_TURN_NOT_FOUND_CODES = [
  AppProtocol.ErrorCode.ResourceNotFound,
  AppProtocol.ErrorCode.SessionNotFound,
] as const satisfies readonly AppProtocol.ErrorCode[];

export function useChatSession(
  sessionId: string,
  serverMessageCount?: number,
  managedWorkerPhase?: AppProtocol.NodeInfo["managedWorkerPhase"],
  sendPolicy: AppProtocol.SessionMessageSendPolicy = "queue",
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
  const activeTurnQuery = useActiveTurnQuery(
    sessionId,
    sessionAvailable && !streamState?.isStreaming,
  );
  const activeTurn = activeTurnQuery.data?.activeTurn;
  const activeTurnReadError = sessionAvailable && !streamState?.isStreaming ? activeTurnQuery.error : null;
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
    managedWorkerPhase,
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
    const error = activeTurnQuery.error;
    if (activeTurn === null && !useAppStore.getState().streams[sessionId]?.isStreaming) {
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
  }, [activeTurn, activeTurnQuery.error, sessionId]);

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
      return Promise.reject(new TalosApiError({ code: AppProtocol.ErrorCode.Conflict, message: "Context is compacting. Wait for it to finish before sending.", localization: null }));
    }
    if (activeTurnId || isStreaming) {
      return messageQueue.add(buildQueuedInput(content, options)).then(() => undefined);
    }
    const request = options
      ? createInitialTurnRequest(
          sessionId,
          content,
          {
            ...(options.nodeId ? { nodeId: options.nodeId } : {}),
            fileIds,
            skillSelection: options.skillSelection,
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
    sendTurn: async (options?: SendTextOptions, promptOverride?: string) => {
      const text = (promptOverride ?? draft).trim();
      const fileIds = options?.fileIds ?? [];
      if (!text && fileIds.length === 0) {
        return;
      }
      if (activeContextCompaction(useAppStore.getState().streams[sessionId])) {
        throw new TalosApiError({ code: AppProtocol.ErrorCode.Conflict, message: "Context is compacting. Wait for it to finish before sending.", localization: null });
      }
      if (activeTurnId || isStreaming) {
        const input = buildQueuedInput(text, options);
        if (
          activeTurnId && !isCancelling && !streamState?.pendingUserAction &&
          (streamState?.isStreaming || activeTurn?.state === "running") &&
          !options?.skillSelection && sendPolicy === "steer"
        ) {
          await submitTurnInput({
            identity: { sessionId, turnId: activeTurnId },
            input: { type: "message", input },
            clientRequestId: null,
          });
        } else {
          await messageQueue.add(input);
        }
        if (useAppStore.getState().composerDrafts[draftKey] === draft) setDraft("");
        return;
      }
      const request = options
        ? createInitialTurnRequest(
            sessionId,
            text,
            {
              ...(options.nodeId ? { nodeId: options.nodeId } : {}),
              fileIds,
              skillSelection: options.skillSelection,
              modelSelection: options.modelSelection,
              project: options.project,
              userPermissionMode: options.userPermissionMode,
            },
          )
        : undefined;
      return sendMutation.mutateAsync({ text, request }).then(() => {
        // A rejected admission keeps the original draft and one-shot Skill mode.
        if (useAppStore.getState().composerDrafts[draftKey] === draft) setDraft("");
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
    automationTurn: automationTurnTiming(sessionId, activeTurnId, streamState, activeTurn),
    messageQueue: {
      ...messageQueue,
      activeTurnId,
      canStart: !isStreaming && !activeTurnId && !streamState?.pendingUserAction,
      canSteer: !streamState?.pendingUserAction && activeTurn?.state !== "waitingForInput",
      start: async (messageId?: string) => {
        const response = await messageQueue.start(messageId);
        await activeTurnQuery.refetch();
        return response;
      },
    },
    pendingUserAction: streamState?.pendingUserAction,
    streamConnectionState: streamState?.connectionState ?? "idle",
    showReconnectControls: Boolean(activeTurnReadError) || (streamState?.showReconnectControls ?? false),
    streamError: activeTurnReadError
      ? userVisibleErrorMessage(activeTurnReadError, "Running Turn query failed.")
      : streamState?.error ? apiErrorMessage(streamState.error) : undefined,
    sessionHistoryQuery,
  };
}
