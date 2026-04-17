import { useEffect, useMemo, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { cancelActiveTurn } from "@/features/chat/api";
import { chatKeys, useConnectionKey, useSessionTurnsQuery } from "@/features/chat/hooks/useChatQueries";
import { submitToolApproval } from "@/features/chat/api";
import { useSessionMessageStream } from "@/shared/sse/session-stream";
import { useAppStore } from "@/shared/store/app-store";
import { mapTurnsToBlocks } from "@/shared/types/domain";

export function useChatSession(sessionId: string, serverMessageCount?: number) {
  const connectionKey = useConnectionKey();
  const queryClient = useQueryClient();
  const [draft, setDraft] = useState("");
  const turnsQuery = useSessionTurnsQuery(sessionId);
  const streamState = useAppStore((state) => state.streams[sessionId]);
  const setSessionStatus = useAppStore((state) => state.setSessionStatus);
  const resetStream = useAppStore((state) => state.resetStream);
  const appendStreamUserInput = useAppStore((state) => state.appendStreamUserInput);
  const pendingApproval = streamState?.pendingApproval;
  const { streamMessage, cancelStream, hasActiveConnection } = useSessionMessageStream(sessionId);

  useEffect(() => {
    return () => {
      cancelStream();
    };
  }, [cancelStream]);

  const stopMutation = useMutation({
    mutationFn: async () => {
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "cancelling",
      });
      await cancelActiveTurn(sessionId);
      cancelStream();
    },
    onSuccess: async () => {
      useAppStore.getState().setPendingApproval(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, undefined);
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
        expectedMessageCount: undefined,
      });
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
    },
    onError: (error) => {
      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to stop response.",
      );
      useAppStore.getState().setSessionStatus(sessionId, "error");
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: false,
        connectionState: "idle",
      });
    },
  });

  const sendMutation = useMutation({
    mutationFn: async (text: string) => {
      setDraft("");
      resetStream(sessionId);
      appendStreamUserInput(sessionId, text);
      useAppStore.getState().setStreamError(sessionId, undefined);
      useAppStore.getState().setStreamMeta(sessionId, {
        isStreaming: true,
        connectionState: "awaiting",
        expectedMessageCount:
          typeof serverMessageCount === "number" ? serverMessageCount + 1 : undefined,
      });
      setSessionStatus(sessionId, "running");
      await streamMessage(text);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      resetStream(sessionId);
      setSessionStatus(sessionId, undefined);
    },
    onError: async (error) => {
      const currentStream = useAppStore.getState().streams[sessionId];
      const errorName = error instanceof Error ? error.name : "";
      const isAbort = errorName === "AbortError";
      const isCancelling = currentStream?.connectionState === "cancelling";
      if (isCancelling) {
        return;
      }

      if (isAbort || currentStream?.isStreaming) {
        useAppStore.getState().setStreamError(sessionId, undefined);
        useAppStore.getState().setStreamMeta(sessionId, {
          isStreaming: true,
          connectionState: "detached",
        });
        setSessionStatus(sessionId, "running");
        await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
        await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
        return;
      }

      useAppStore.getState().setStreamError(
        sessionId,
        error instanceof Error ? error.message : "Failed to stream message.",
      );
      setSessionStatus(sessionId, "error");
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
    },
  });

  const approvalMutation = useMutation({
    mutationFn: async (approved: boolean) => {
      if (!pendingApproval) {
        throw new Error("No pending approval.");
      }
      await submitToolApproval(sessionId, {
        call_id: pendingApproval.callId,
        tool_name: pendingApproval.toolName,
        approved,
      });
      useAppStore.getState().setPendingApproval(sessionId, undefined);
      useAppStore.getState().setSessionStatus(sessionId, "running");
    },
  });

  useEffect(() => {
    if (!streamState?.isStreaming || streamState.pendingApproval) {
      return;
    }

    const timeoutId = window.setTimeout(() => {
      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming || currentStream.pendingApproval) {
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
      });
      useAppStore.getState().setSessionStatus(sessionId, "running");
    }, 350);

    return () => window.clearTimeout(timeoutId);
  }, [
    hasActiveConnection,
    sessionId,
    streamState?.connectionState,
    streamState?.isStreaming,
    streamState?.pendingApproval,
  ]);

  useEffect(() => {
    if (
      !streamState?.isStreaming ||
      typeof streamState.expectedMessageCount !== "number" ||
      typeof serverMessageCount !== "number" ||
      serverMessageCount < streamState.expectedMessageCount
    ) {
      return;
    }

    void queryClient
      .invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) })
      .then(() => {
        useAppStore.getState().setPendingApproval(sessionId, undefined);
        useAppStore.getState().setSessionStatus(sessionId, undefined);
        resetStream(sessionId);
      });
  }, [
    connectionKey,
    queryClient,
    resetStream,
    serverMessageCount,
    sessionId,
    streamState?.expectedMessageCount,
    streamState?.isStreaming,
  ]);

  useEffect(() => {
    if (
      !streamState?.isStreaming ||
      !(
        streamState.connectionState === "detached" ||
        streamState.connectionState === "reconnecting"
      )
    ) {
      return;
    }

    let disposed = false;
    const refresh = async () => {
      await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
      await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });

      if (disposed) {
        return;
      }

      const currentStream = useAppStore.getState().streams[sessionId];
      if (!currentStream?.isStreaming) {
        return;
      }
      if (currentStream.connectionState === "reconnecting") {
        useAppStore.getState().setStreamMeta(sessionId, {
          connectionState: "detached",
        });
      }
    };

    void refresh();
    const intervalId = window.setInterval(() => {
      void refresh();
    }, 4_000);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
    };
  }, [
    connectionKey,
    queryClient,
    sessionId,
    streamState?.connectionState,
    streamState?.isStreaming,
  ]);

  const retryStream = async () => {
    useAppStore.getState().setStreamError(sessionId, undefined);
    useAppStore.getState().setSessionStatus(sessionId, "running");
    useAppStore.getState().setStreamMeta(sessionId, {
      isStreaming: true,
      connectionState: "reconnecting",
    });
    await queryClient.invalidateQueries({ queryKey: chatKeys.sessions(connectionKey) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.session(connectionKey, sessionId) });
    await queryClient.invalidateQueries({ queryKey: chatKeys.turns(connectionKey, sessionId) });
  };

  const blocks = useMemo(() => {
    const historyBlocks = mapTurnsToBlocks(turnsQuery.data ?? []);
    const liveBlocks = streamState?.blocks ?? [];
    const shouldShowLiveBlocks =
      liveBlocks.length > 0 ||
      (streamState?.isStreaming ?? false) ||
      Boolean(streamState?.error) ||
      Boolean(streamState?.pendingApproval);
    const nextBlocks = [...historyBlocks, ...(shouldShowLiveBlocks ? liveBlocks : [])];
    const hasLiveResponse = liveBlocks.some(
      (block) =>
        block.kind === "assistant" ||
        block.kind === "thinking" ||
        block.kind === "system" ||
        block.kind === "tool",
    );
    const shouldShowWaitingPlaceholder =
      Boolean(streamState?.isStreaming) &&
      !streamState?.pendingApproval &&
      !streamState?.error &&
      !hasLiveResponse &&
      streamState?.connectionState !== "cancelling" &&
      streamState?.connectionState !== "detached" &&
      streamState?.connectionState !== "reconnecting";

    if (shouldShowWaitingPlaceholder) {
      nextBlocks.push({
        id: `status-awaiting-${sessionId}`,
        kind: "status",
        variant: "thinking",
        title: "Waiting for server response",
        detail: "Talos is still preparing the first reply chunk.",
        live: true,
      });
    }

    if (streamState?.connectionState === "cancelling") {
      nextBlocks.push({
        id: `status-cancelling-${sessionId}`,
        kind: "status",
        variant: "cancelling",
        title: "Stopping response…",
        detail: "Sending a stop request to the server.",
        live: true,
      });
    }

    return nextBlocks;
  }, [
    sessionId,
    streamState?.blocks,
    streamState?.connectionState,
    streamState?.error,
    streamState?.isStreaming,
    streamState?.pendingApproval,
    turnsQuery.data,
  ]);

  return {
    blocks,
    draft,
    setDraft,
    sendText: (text: string) => {
      const content = text.trim();
      if (!content) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync(content);
    },
    sendMessage: () => {
      const text = draft.trim();
      if (!text) {
        return Promise.resolve();
      }
      return sendMutation.mutateAsync(text);
    },
    retryStream,
    stopResponse: () => stopMutation.mutateAsync(),
    submitApproval: (approved: boolean) => approvalMutation.mutateAsync(approved),
    isStreaming: streamState?.isStreaming ?? false,
    isCancelling: stopMutation.isPending || streamState?.connectionState === "cancelling",
    streamConnectionState: streamState?.connectionState ?? "idle",
    pendingApproval,
    streamError: streamState?.error,
    turnsQuery,
  };
}
