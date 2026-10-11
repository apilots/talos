import { useEffect, useRef, useState } from "react";
import { AppProtocol, TalosApiError, TalosStreamTerminatedError, type SessionHistoryResponse } from "@talos/sdk-browser";

import { recoverTimelineLiveTail, subscribeTimelineEvents } from "../api";
import { applyStreamEvent, applyTimelineRecovery, clearInactiveTurnSnapshot } from "../stream-events";
import type { useSessionMessageQueue } from "./useSessionMessageQueue";
import { useConnectionKey } from "./useChatQueries";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleError } from "@/shared/i18n/apiErrorMessage";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;
const EVENT_CURSOR_EXPIRED_CODE = AppProtocol.ErrorCode.EventCursorExpired;

export function useSessionTimelineEvents(
  sessionId: string,
  afterEventId: string | undefined,
  enabled: boolean,
  onTerminalTurn: (() => void | Promise<void>) | undefined,
  queue: Pick<ReturnType<typeof useSessionMessageQueue>, "onOpen" | "onDisconnect" | "onMessageQueueChanged"> | undefined,
  historyOptions: {
    stableEventSeq: number;
    reloadHistory: () => Promise<SessionHistoryResponse>;
  },
) {
  const scope = useConnectionKey();
  const [retryGeneration, setRetryGeneration] = useState(0);
  const onTerminalTurnRef = useRef(onTerminalTurn);
  onTerminalTurnRef.current = onTerminalTurn;
  const queueRef = useRef(queue);
  queueRef.current = queue;
  const historyOptionsRef = useRef(historyOptions);
  historyOptionsRef.current = historyOptions;
  const afterEventIdRef = useRef(afterEventId);
  afterEventIdRef.current = afterEventId;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let disposed = false;
    let controller = new AbortController();
    let reconnectTimer: number | undefined;
    let finishReconnectDelay: (() => void) | undefined;
    const stream = useAppStore.getState().streams[sessionId];
    const stableEventSeq = historyOptionsRef.current.stableEventSeq;
    let subscriptionCursor = (stream?.lastEventSeq ?? 0) >= stableEventSeq
      ? stream?.lastEventId ?? afterEventIdRef.current
      : afterEventIdRef.current;

    const subscribe = async () => {
      for (let attempt = 0; !disposed; attempt += 1) {
        controller = new AbortController();
        if (attempt > 0 && useAppStore.getState().streams[sessionId]?.isStreaming) {
          useAppStore.getState().setStreamMeta(sessionId, {
            connectionState: "reconnecting",
            reconnectAttempt: attempt,
            showReconnectControls: false,
          });
        }
        let reconciliationError: unknown;
        try {
          await subscribeTimelineEvents(
            sessionId,
            (event) => {
              if (disposed || connectionScope(useAppStore.getState().connection) !== scope) return;
              subscriptionCursor = event.eventId;
              if (event.eventSeq <= (historyOptionsRef.current.stableEventSeq)) return;
              applyStreamEvent(sessionId, event);
              if (
                event.event.type === "systemNotice" ||
                (event.event.type === "agent" && (event.event.data.payload.type === "turnCompleted" ||
                event.event.data.payload.type === "turnFailed"))
              ) {
                void onTerminalTurnRef.current?.();
              }
            },
            controller.signal,
            subscriptionCursor,
            {
              onOpen: () => queueRef.current?.onOpen((error) => {
                reconciliationError = error;
                controller.abort();
              }),
              onMessageQueueChanged: (change) => queueRef.current?.onMessageQueueChanged(change),
            },
          );
          throw new Error("Session event subscription closed.");
        } catch (caught: unknown) {
          const error = reconciliationError ?? caught;
          if (disposed || (error instanceof Error && error.name === "AbortError")) {
            return;
          }
          queueRef.current?.onDisconnect();
          let subscriptionError = error;
          const cursorExpired = error instanceof TalosApiError && error.code === EVENT_CURSOR_EXPIRED_CODE;
          const lagged = error instanceof TalosStreamTerminatedError && error.reason === "lagged";
          if (cursorExpired || lagged) {
            try {
              const history = cursorExpired
                ? await historyOptionsRef.current.reloadHistory()
                : undefined;
              if (disposed || connectionScope(useAppStore.getState().connection) !== scope) return;
              if (history) subscriptionCursor = history.lastStableEventId ?? undefined;
              const recovery = await recoverTimelineLiveTail(sessionId, subscriptionCursor);
              if (disposed || connectionScope(useAppStore.getState().connection) !== scope) return;
              if (history) {
                clearInactiveTurnSnapshot(sessionId);
                useAppStore.getState().resetLiveResponse(sessionId);
                useAppStore.getState().setStreamMeta(sessionId, {
                  lastEventId: subscriptionCursor,
                  lastEventSeq: history.data.at(-1)?.completedEventSeq ?? 0,
                  lastSequence: undefined,
                });
              }
              useAppStore.getState().setStreamError(sessionId, undefined);
              applyTimelineRecovery(sessionId, recovery);
              subscriptionCursor =
                recovery.replay.nextCursor ??
                recovery.replay.highWaterMarkEventId ??
                subscriptionCursor;
              await onTerminalTurnRef.current?.();
              // The lifecycle owner stops idle subscriptions after reconciliation.
              continue;
            } catch (recoveryError: unknown) {
              subscriptionError = recoveryError;
            }
          }
          const exhausted = attempt + 1 >= RECONNECT_DELAYS_MS.length;
          const state = useAppStore.getState();
          state.setStreamError(
            sessionId,
            userVisibleError(subscriptionError, "Session event subscription failed."),
          );
          if (state.streams[sessionId]?.isStreaming) {
            state.setStreamMeta(sessionId, {
              connectionState: "detached",
              reconnectAttempt: attempt + 1,
              showReconnectControls: exhausted,
            });
          }
          await new Promise<void>((resolve) => {
            finishReconnectDelay = resolve;
            reconnectTimer = window.setTimeout(() => {
              finishReconnectDelay = undefined;
              resolve();
            }, RECONNECT_DELAYS_MS[Math.min(attempt, RECONNECT_DELAYS_MS.length - 1)]);
          });
        }
      }
    };

    void subscribe();
    return () => {
      disposed = true;
      controller.abort();
      queueRef.current?.onDisconnect();
      if (reconnectTimer !== undefined) {
        window.clearTimeout(reconnectTimer);
      }
      finishReconnectDelay?.();
    };
  }, [scope, enabled, retryGeneration, sessionId]);

  return {
    retryTimeline: () => setRetryGeneration((generation) => generation + 1),
  };
}
