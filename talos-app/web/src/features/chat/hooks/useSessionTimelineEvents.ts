import { useEffect, useState } from "react";

import { subscribeTimelineEvents } from "../api";
import { applyStreamEvent } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;

export function useSessionTimelineEvents(
  sessionId: string,
  afterEventId: string | undefined,
  enabled: boolean,
) {
  const [retryGeneration, setRetryGeneration] = useState(0);

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
            },
            controller.signal,
            useAppStore.getState().streams[sessionId]?.lastEventId ?? afterEventId,
          );
          return;
        } catch (error: unknown) {
          if (disposed || (error instanceof Error && error.name === "AbortError")) {
            return;
          }
          const exhausted = attempt + 1 >= RECONNECT_DELAYS_MS.length;
          const state = useAppStore.getState();
          state.setStreamError(
            sessionId,
            error instanceof Error ? error.message : "Session event subscription failed.",
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
