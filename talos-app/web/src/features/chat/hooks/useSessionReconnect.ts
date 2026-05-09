import { useEffect } from "react";

import { useSessionQueryInvalidation } from "./useSessionQueryInvalidation";
import { useAppStore } from "@/shared/store/app-store";
import type { StreamSessionState } from "@/shared/types/ui";

const RECONNECT_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 12_000] as const;

function reconnectDelayMs(attempt: number) {
  return RECONNECT_DELAYS_MS[Math.min(attempt, RECONNECT_DELAYS_MS.length - 1)];
}

export function useSessionReconnect({
  hasActiveConnection,
  reconnectMessage,
  sessionId,
  streamState,
}: {
  hasActiveConnection: () => boolean;
  reconnectMessage: (messageId: string) => Promise<void>;
  sessionId: string;
  streamState?: StreamSessionState;
}) {
  const { invalidateSessionQueries } = useSessionQueryInvalidation(sessionId);

  useEffect(() => {
    if (!streamState?.isStreaming || streamState.pendingUserAction || streamState.pendingApproval) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming || currentStream.pendingUserAction || currentStream.pendingApproval) {
        return;
      }
      if (currentStream.connectionState === "cancelling") {
        return;
      }
      if (hasActiveConnection()) {
        return;
      }
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "detached",
        reconnectAttempt: 0,
        showReconnectControls: false,
      });
      useAppStore.getState().setSessionStatus(sessionId, "running");
    }, 350);

    return () => window.clearTimeout(timeoutId);
  }, [
    hasActiveConnection,
    sessionId,
    streamState?.connectionState,
    streamState?.isStreaming,
    streamState?.pendingUserAction,
    streamState?.pendingApproval,
  ]);

  useEffect(() => {
    if (
      !streamState?.isStreaming ||
      !(streamState.connectionState === "detached" || streamState.connectionState === "reconnecting")
    ) {
      return;
    }

    let disposed = false;
    const attempt = streamState.reconnectAttempt ?? 0;
    const refresh = async () => {
      await invalidateSessionQueries();

      if (disposed) {
        return;
      }

      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming) {
        return;
      }
      if (hasActiveConnection()) {
        return;
      }
      if ((currentStream.reconnectAttempt ?? 0) >= RECONNECT_DELAYS_MS.length) {
        useAppStore.getState().setStreamMeta(sessionId, {
          connectionState: "detached",
          showReconnectControls: true,
        });
        return;
      }
      if (
        currentStream.activeMessageId &&
        !currentStream.pendingUserAction &&
        !currentStream.pendingApproval
      ) {
        const nextAttempt = (currentStream.reconnectAttempt ?? 0) + 1;
        useAppStore.getState().setStreamMeta(sessionId, {
          connectionState: "reconnecting",
          reconnectAttempt: nextAttempt,
          showReconnectControls: false,
        });
        try {
          await reconnectMessage(currentStream.activeMessageId);
          return;
        } catch {
          if (disposed) {
            return;
          }
          const failedStream = useAppStore.getState().streams[sessionId];
          const exhausted = (failedStream?.reconnectAttempt ?? nextAttempt) >= RECONNECT_DELAYS_MS.length;
          useAppStore.getState().setStreamMeta(sessionId, {
            connectionState: "detached",
            showReconnectControls: exhausted,
          });
          return;
        }
      }
      if (currentStream.connectionState === "reconnecting") {
        const exhausted = (currentStream.reconnectAttempt ?? 0) >= RECONNECT_DELAYS_MS.length;
        useAppStore.getState().setStreamMeta(sessionId, {
          connectionState: "detached",
          showReconnectControls: exhausted,
        });
      }
    };
    const timeoutId = window.setTimeout(() => {
      void refresh();
    }, reconnectDelayMs(attempt));

    return () => {
      disposed = true;
      window.clearTimeout(timeoutId);
    };
  }, [
    hasActiveConnection,
    invalidateSessionQueries,
    reconnectMessage,
    sessionId,
    streamState?.connectionState,
    streamState?.isStreaming,
    streamState?.reconnectAttempt,
  ]);

  return {
    retryStream: async () => {
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, "running");
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "reconnecting",
        reconnectAttempt: 0,
        showReconnectControls: false,
      });
      const currentStream = useAppStore.getState().streams[sessionId];
      if (currentStream?.activeMessageId) {
        await reconnectMessage(currentStream.activeMessageId);
        return;
      }
      await invalidateSessionQueries();
    },
  };
}
