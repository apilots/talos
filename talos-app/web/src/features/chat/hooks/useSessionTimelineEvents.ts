import { useEffect, useRef, useState } from "react";
import { AppProtocol, TalosApiError, TalosStreamTerminatedError } from "@talos/sdk-browser";

import { recoverTimelineLiveTail, subscribeTimelineEvents } from "../api";
import { applyStreamEvent, applyTimelineRecovery } from "../stream-events";
import type { useSessionMessageQueue } from "./useSessionMessageQueue";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleError } from "@/shared/i18n/apiErrorMessage";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;
const EVENT_CURSOR_EXPIRED_CODE = AppProtocol.ErrorCode.EventCursorExpired;

export function useSessionTimelineEvents(
  sessionId: string,
  afterEventId: string | undefined,
  enabled: boolean,
  onTerminalTurn?: () => void | Promise<void>,
  queue?: Pick<ReturnType<typeof useSessionMessageQueue>, "onOpen" | "onDisconnect" | "onMessageQueueChanged">,
) {
  const [retryGeneration, setRetryGeneration] = useState(0);
  const onTerminalTurnRef = useRef(onTerminalTurn);
  onTerminalTurnRef.current = onTerminalTurn;
  const queueRef = useRef(queue);
  queueRef.current = queue;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let disposed = false;
    let controller = new AbortController();
    let reconnectTimer: number | undefined;
    let finishReconnectDelay: (() => void) | undefined;
    let subscriptionCursor =
      useAppStore.getState().streams[sessionId]?.lastEventId ?? afterEventId;

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
              subscriptionCursor = event.eventId;
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
              const recovery = cursorExpired
                ? await recoverTimelineLiveTail(sessionId)
                : await recoverTimelineLiveTail(sessionId, subscriptionCursor);
              if (disposed) return;
              useAppStore.getState().setStreamError(sessionId, undefined);
              applyTimelineRecovery(sessionId, recovery);
              subscriptionCursor =
                recovery.replay.nextCursor ??
                recovery.replay.highWaterMarkEventId ??
                subscriptionCursor;
              await onTerminalTurnRef.current?.();
              // The Session subscription must also survive an idle recovery:
              // another client or the queue can start its next Turn.
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
  }, [afterEventId, enabled, retryGeneration, sessionId]);

  return {
    retryTimeline: () => setRetryGeneration((generation) => generation + 1),
  };
}
