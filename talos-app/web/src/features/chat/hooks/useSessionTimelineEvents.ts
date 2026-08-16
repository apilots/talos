import { useEffect, useRef, useState } from "react";
import { AppProtocol, TalosApiError, TalosStreamTerminatedError } from "@talos/sdk-browser";

import { recoverTimelineLiveTail, subscribeTimelineEvents } from "../api";
import { applyStreamEvent, applyTimelineRecovery } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;
const EVENT_CURSOR_EXPIRED_CODE = AppProtocol.ErrorCode.EventCursorExpired;

export function useSessionTimelineEvents(
  sessionId: string,
  afterEventId: string | undefined,
  enabled: boolean,
  onTerminalTurn?: () => void | Promise<void>,
) {
  const [retryGeneration, setRetryGeneration] = useState(0);
  const onTerminalTurnRef = useRef(onTerminalTurn);
  onTerminalTurnRef.current = onTerminalTurn;

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let disposed = false;
    let controller = new AbortController();
    let reconnectTimer: number | undefined;
    let finishReconnectDelay: (() => void) | undefined;

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
        try {
          await subscribeTimelineEvents(
            sessionId,
            (event) => {
              useAppStore.getState().setStreamError(sessionId, undefined);
              applyStreamEvent(sessionId, event);
              if (
                event.event.payload.type === "turn_completed" &&
                event.event.payload.stop_reason !== "waiting_user_action"
              ) {
                void onTerminalTurnRef.current?.();
              }
            },
            controller.signal,
            useAppStore.getState().streams[sessionId]?.lastEventId ?? afterEventId,
          );
          return;
        } catch (error: unknown) {
          if (disposed || (error instanceof Error && error.name === "AbortError")) {
            return;
          }
          let subscriptionError = error;
          if (
            error instanceof TalosApiError &&
            error.code === EVENT_CURSOR_EXPIRED_CODE
          ) {
            try {
              const recovery = await recoverTimelineLiveTail(sessionId);
              applyTimelineRecovery(sessionId, recovery);
              const state = useAppStore.getState();
              state.setStreamError(sessionId, undefined);
              if (!state.streams[sessionId]?.isStreaming) {
                return;
              }
              continue;
            } catch (recoveryError: unknown) {
              subscriptionError = recoveryError;
            }
          }
          if (error instanceof TalosStreamTerminatedError && error.reason === "lagged") {
            try {
              const cursor = useAppStore.getState().streams[sessionId]?.lastEventId ?? afterEventId;
              const recovery = await recoverTimelineLiveTail(sessionId, cursor);
              applyTimelineRecovery(sessionId, recovery);
              const state = useAppStore.getState();
              state.setStreamError(sessionId, undefined);
              if (!state.streams[sessionId]?.isStreaming) {
                return;
              }
              continue;
            } catch (recoveryError: unknown) {
              subscriptionError = recoveryError;
            }
          }
          const exhausted = attempt + 1 >= RECONNECT_DELAYS_MS.length;
          const state = useAppStore.getState();
          state.setStreamError(
            sessionId,
            subscriptionError instanceof Error
              ? subscriptionError.message
              : "Session event subscription failed.",
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
