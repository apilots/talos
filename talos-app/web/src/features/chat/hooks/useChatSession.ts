import { useEffect, useState } from "react";

import { useSessionApproval } from "./useSessionApproval";
import { useSessionBlocks } from "./useSessionBlocks";
import { useSessionReconnect } from "./useSessionReconnect";
import { useSessionStreaming } from "./useSessionStreaming";
import { useActiveTurnQuery, useSessionTurnsQuery } from "./useChatQueries";
import { applyActiveTurnSnapshot } from "../stream-events";
import { useAppStore } from "@/shared/store/app-store";

export function useChatSession(sessionId: string, serverMessageCount?: number) {
  const [draft, setDraft] = useState("");
  const activeTurnQuery = useActiveTurnQuery(sessionId);
  const turnsQuery = useSessionTurnsQuery(sessionId);
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const {
    continueAfterApproval,
    hasActiveConnection,
    isCancelling,
    isStreaming,
    reconnectMessage,
    retryResponse,
    sendMutation,
    sendText,
    stopResponse,
    streamState: liveStreamState,
    canRetryResponse,
  } =
    useSessionStreaming(sessionId, serverMessageCount);
  const { pendingApproval, submitApproval } = useSessionApproval(sessionId, continueAfterApproval);
  const { retryStream } = useSessionReconnect({
    hasActiveConnection,
    reconnectMessage,
    serverMessageCount,
    sessionId,
    streamState: liveStreamState,
  });
  const blocks = useSessionBlocks({
    sessionId,
    streamState,
    turns: turnsQuery.data,
  });

  useEffect(() => {
    if (!activeTurnQuery.data) {
      return;
    }
    applyActiveTurnSnapshot(sessionId, activeTurnQuery.data);
  }, [activeTurnQuery.data, sessionId]);

  return {
    blocks,
    draft,
    setDraft,
    sendText,
    sendMessage: () => {
      const text = draft.trim();
      if (!text) {
        return Promise.resolve();
      }
      setDraft("");
      return sendMutation.mutateAsync({ text, appendUserInput: true });
    },
    retryStream,
    retryResponse,
    canRetryResponse,
    stopResponse,
    submitApproval,
    isStreaming,
    isCancelling,
    streamConnectionState: streamState?.connectionState ?? "idle",
    showReconnectControls: streamState?.showReconnectControls ?? false,
    pendingApproval,
    streamError: streamState?.error,
    turnsQuery,
  };
}
