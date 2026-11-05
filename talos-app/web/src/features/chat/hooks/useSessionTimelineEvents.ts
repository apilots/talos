import { useEffect, useRef, useState } from "react";
import { AppProtocol, TalosApiError, TalosStreamTerminatedError, type SessionActiveTurnResponse, type SessionHistoryResponse } from "@talos/sdk-browser";

import { getActiveTurn, subscribeTimelineEvents } from "../api";
import { applyStreamEventBatch, restoreActiveTurn, applyActiveTurnSnapshot, clearInactiveTurnSnapshot } from "../stream-events";
import type { useSessionMessageQueue } from "./useSessionMessageQueue";
import { useConnectionKey } from "./useChatQueries";
import { connectionScope } from "@/shared/store/connection-scope";
import { useAppStore } from "@/shared/store/app-store";
import { userVisibleError } from "@/shared/i18n/apiErrorMessage";

import { createTimelineEventBatcher } from "../timeline-event-batcher";

const RECONNECT_DELAY_MS = 2_000;
const EVENT_CURSOR_EXPIRED_CODE = AppProtocol.ErrorCode.EventCursorExpired;

export function useSessionTimelineEvents(
  sessionId: string,
  afterEventId: string | undefined,
  enabled: boolean,
  onTerminalTurn: (() => void | Promise<void>) | undefined,
  queue: Pick<ReturnType<typeof useSessionMessageQueue>, "onOpen" | "onDisconnect" | "onMessageQueueChanged"> | undefined,
  historyOptions: {
    activeTurnState?: SessionActiveTurnResponse;
    history: SessionHistoryResponse | undefined;
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
    let batcher: ReturnType<typeof createTimelineEventBatcher> | undefined;
    const stream = useAppStore.getState().streams[sessionId];
    const stableEventSeq = historyOptionsRef.current.stableEventSeq;
    let subscriptionCursor = (stream?.lastEventSeq ?? 0) >= stableEventSeq
      ? stream?.lastEventId ?? afterEventIdRef.current
      : afterEventIdRef.current;

    const initialState = historyOptionsRef.current.activeTurnState;
    const initialSnapshot = initialState?.activeTurn;
    if (initialSnapshot && stream?.activeTurnId === initialSnapshot.identity.turnId) restoreActiveTurn(sessionId, initialSnapshot);
    // A cached idle query must not rewind an already committed local stream.
    let needsBootstrap = initialSnapshot !== undefined &&
      (initialSnapshot === null ? !stream?.isStreaming :
        !stream?.lastEventId || stream.activeTurnId !== initialSnapshot.identity.turnId ||
        (stream.lastEventSeq ?? 0) < initialSnapshot.lastEventSeq);
    const reconcileHistory = async (current: SessionActiveTurnResponse, history: SessionHistoryResponse) => {
      const coversCompleted = (value: SessionHistoryResponse) =>
        (value.data.at(-1)?.completedEventSeq ?? 0) >= current.lastCompletedEventSeq;
      if (!coversCompleted(history)) {
        // Only independent reads that straddle a completion need another read.
        history = await historyOptionsRef.current.reloadHistory();
        if (!coversCompleted(history)) throw new Error("Session history does not cover the running Turn read.");
      }
      return history;
    };
    const subscribe = async () => {
      for (let attempt = 0; !disposed; attempt += 1) {
        const subscriptionController = new AbortController();
        controller = subscriptionController;
        if (attempt > 0 && useAppStore.getState().streams[sessionId]?.isStreaming) {
          useAppStore.getState().setStreamMeta(sessionId, {
            connectionState: "reconnecting",
            reconnectAttempt: attempt,
            showReconnectControls: false,
          });
        }
        let reconciliationError: unknown;
        try {
          if (needsBootstrap) {
            const loadedHistory = historyOptionsRef.current.history;
            if (!loadedHistory) throw new Error("Session history must load before restoring the running Turn.");
            const history = await reconcileHistory(initialState!, loadedHistory);
            if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
            const completed = !initialSnapshot || history.data.some((turn) => turn.identity.turnId === initialSnapshot.identity.turnId);
            if (completed) subscriptionCursor = history.lastStableEventId ?? undefined;
            else if (initialSnapshot) {
              restoreActiveTurn(sessionId, initialSnapshot);
              subscriptionCursor = initialSnapshot.lastEventId;
            }
            needsBootstrap = false;
          }
          const attemptBatcher = createTimelineEventBatcher((events) => {
            if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return events.length;
            const accepted = events.filter((event) => event.eventSeq > historyOptionsRef.current.stableEventSeq);
            const consumed = applyStreamEventBatch(sessionId, accepted, 8);
            const applied = accepted.slice(0, consumed);
            const last = applied.at(-1);
            if (last) {
              const current = useAppStore.getState().streams[sessionId];
              subscriptionCursor = (current?.lastEventSeq ?? 0) >= last.eventSeq
                ? current?.lastEventId ?? last.eventId : last.eventId;
            }
            for (const event of applied) {
              if (event.event.type === "systemNotice" || (event.event.type === "agent" &&
                  (event.event.data.payload.type === "turnCompleted" || event.event.data.payload.type === "turnFailed"))) {
                void onTerminalTurnRef.current?.();
              }
            }
            // Filtered stable history events need no state mutation.
            return consumed === accepted.length ? events.length : events.indexOf(applied.at(-1)!) + 1;
          }, (error) => { reconciliationError = error; subscriptionController.abort(); });
          batcher = attemptBatcher;
          await subscribeTimelineEvents(
            sessionId,
            (event) => attemptBatcher.push(event),
            subscriptionController.signal,
            subscriptionCursor,
            {
              onOpen: () => {
                if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
                queueRef.current?.onOpen((error) => {
                  if (disposed || subscriptionController.signal.aborted) return;
                  reconciliationError = error;
                  subscriptionController.abort();
                });
                // Skill registration can bind provenance after the initial read,
                // without advancing the canonical event offset.
                void (async () => {
                  await Promise.all([
                    // Definitions must not wait for the full running-Turn read.
                    onTerminalTurnRef.current?.(),
                    getActiveTurn(sessionId).then((current) => {
                      if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
                      if (current.activeTurn) restoreActiveTurn(sessionId, current.activeTurn);
                    }),
                  ]);
                })().catch((error) => { reconciliationError = error; subscriptionController.abort(); });
              },
              onActiveTurnChanged: (notice) => {
                if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
                if (notice.state.activeTurn) applyActiveTurnSnapshot(sessionId, notice.state.activeTurn);
                else clearInactiveTurnSnapshot(sessionId);
                void onTerminalTurnRef.current?.();
              },
              onAutomationChanged: (change) => {
                if (change.sessionId !== sessionId || disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
                void (async () => {
                  await Promise.all([
                    onTerminalTurnRef.current?.(),
                    getActiveTurn(sessionId).then((current) => {
                      if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
                      if (current.activeTurn) restoreActiveTurn(sessionId, current.activeTurn);
                    }),
                  ]);
                })().catch((error) => { reconciliationError = error; subscriptionController.abort(); });
              },
              onMessageQueueChanged: (change) => queueRef.current?.onMessageQueueChanged(change),
            },
          );
          throw new Error("Session event subscription closed.");
        } catch (caught: unknown) {
          batcher?.cancel();
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
              const [current, loadedHistory] = await Promise.all([
                getActiveTurn(sessionId), historyOptionsRef.current.reloadHistory(),
              ]);
              if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
              const history = await reconcileHistory(current, loadedHistory);
              if (disposed || subscriptionController.signal.aborted || connectionScope(useAppStore.getState().connection) !== scope) return;
              clearInactiveTurnSnapshot(sessionId);
              useAppStore.getState().resetLiveResponse(sessionId);
              subscriptionCursor = history.lastStableEventId ?? undefined;
              useAppStore.getState().setStreamMeta(sessionId, {
                lastEventId: subscriptionCursor,
                lastEventSeq: history.data.at(-1)?.completedEventSeq ?? 0,
                lastSequence: undefined,
              });
              if (current.activeTurn && !history.data.some((turn) => turn.identity.turnId === current.activeTurn?.identity.turnId)) {
                restoreActiveTurn(sessionId, current.activeTurn);
                subscriptionCursor = current.activeTurn.lastEventId;
              }
              await onTerminalTurnRef.current?.();
              // The lifecycle owner stops idle subscriptions after reconciliation.
              continue;
            } catch (recoveryError: unknown) {
              subscriptionError = recoveryError;
            }
          }
          const exhausted = attempt >= 3;
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
            }, RECONNECT_DELAY_MS);
          });
        }
      }
    };

    void subscribe();
    return () => {
      disposed = true;
      batcher?.cancel();
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
