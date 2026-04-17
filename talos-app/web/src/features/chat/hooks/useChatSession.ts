import { useState } from "react";

import { useSessionApproval } from "./useSessionApproval";
import { useSessionBlocks } from "./useSessionBlocks";
import { useSessionReconnect } from "./useSessionReconnect";
import { useSessionStreaming } from "./useSessionStreaming";
import { useSessionTurnsQuery } from "./useChatQueries";
import { useAppStore } from "@/shared/store/app-store";

export function useChatSession(sessionId: string, serverMessageCount?: number) {
  const [draft, setDraft] = useState("");
  const turnsQuery = useSessionTurnsQuery(sessionId);
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const { pendingApproval, submitApproval } = useSessionApproval(sessionId);
  const { hasActiveConnection, isCancelling, isStreaming, sendMutation, sendText, stopResponse, streamState: liveStreamState } =
    useSessionStreaming(sessionId, serverMessageCount);
  const { retryStream } = useSessionReconnect({
    hasActiveConnection,
    serverMessageCount,
    sessionId,
    streamState: liveStreamState,
  });
  const blocks = useSessionBlocks({
    sessionId,
    streamState,
    turns: turnsQuery.data,
  });

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
      return sendMutation.mutateAsync(text);
    },
    retryStream,
    stopResponse,
    submitApproval,
    isStreaming,
    isCancelling,
    streamConnectionState: streamState?.connectionState ?? "idle",
    pendingApproval,
    streamError: streamState?.error,
    turnsQuery,
  };
}
