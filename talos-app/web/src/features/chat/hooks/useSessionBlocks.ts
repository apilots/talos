import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { mapTurnsToBlocks } from "@/shared/types/domain";
import type { ChatRenderBlock, StreamSessionState } from "@/shared/types/ui";
import type { ChatApiTurns } from "../api";

export function useSessionBlocks({
  sessionId,
  streamState,
  turns,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  turns?: ChatApiTurns;
}) {
  const { t } = useTranslation("chat");

  return useMemo(() => {
    return buildSessionBlocks({
      sessionId,
      streamState,
      turns,
      waitingResponseDetail: t("waitingResponseDetail"),
      waitingResponseTitle: t("waitingResponseTitle"),
      finalResponseWaitDetail: t("finalResponseWaitDetail"),
      finalResponseWaitTitle: t("finalResponseWaitTitle"),
      stoppingResponseDetail: t("stoppingResponseDetail"),
      stoppingResponseTitle: t("stoppingResponseTitle"),
    });
  }, [
    sessionId,
    streamState,
    t,
    turns,
  ]);
}

export function buildSessionBlocks({
  sessionId,
  streamState,
  turns,
  waitingResponseDetail,
  waitingResponseTitle,
  finalResponseWaitDetail = "Talos is composing the final response.",
  finalResponseWaitTitle = "Preparing final response",
  stoppingResponseDetail,
  stoppingResponseTitle,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  turns?: ChatApiTurns;
  waitingResponseDetail: string;
  waitingResponseTitle: string;
  finalResponseWaitDetail?: string;
  finalResponseWaitTitle?: string;
  stoppingResponseDetail: string;
  stoppingResponseTitle: string;
}) {
  const historyBlocks = mapTurnsToBlocks(turns ?? []);
  const historyHasActiveMessage = hasHistoryMessage(turns, streamState?.activeMessageId);
  const historyHasPendingInput = hasHistoryPendingInput(turns, streamState?.pendingInput);
  const pendingInputBlock = historyHasPendingInput ? undefined : createPendingInputBlock(streamState, turns);
  const liveBlocks = historyHasActiveMessage ? [] : streamState?.blocks ?? [];
  const nextBlocks = [
    ...historyBlocks,
    ...(pendingInputBlock ? [pendingInputBlock] : []),
    ...liveBlocks,
  ];
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
    !streamState?.pendingUserAction &&
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
      title: waitingResponseTitle,
      detail: waitingResponseDetail,
      live: true,
    });
  }

  if (
    streamState?.awaitingFinalResponse &&
    !historyHasActiveMessage &&
    !streamState.pendingApproval &&
    !streamState.pendingUserAction &&
    !streamState.error
  ) {
    nextBlocks.push({
      id: `status-final-response-${sessionId}`,
      kind: "status",
      variant: "thinking",
      title: finalResponseWaitTitle,
      detail: finalResponseWaitDetail,
      live: true,
    });
  }

  if (streamState?.connectionState === "cancelling") {
    nextBlocks.push({
      id: `status-cancelling-${sessionId}`,
      kind: "status",
      variant: "cancelling",
      title: stoppingResponseTitle,
      detail: stoppingResponseDetail,
      live: true,
    });
  }

  return nextBlocks;
}

function hasHistoryMessage(turns: ChatApiTurns | undefined, messageId: string | undefined) {
  if (!messageId) {
    return false;
  }
  return (turns ?? []).some((turn) => turn.message_id === messageId);
}

function hasHistoryPendingInput(
  turns: ChatApiTurns | undefined,
  pendingInput: StreamSessionState["pendingInput"],
) {
  if (!pendingInput?.serverMessageId) {
    return false;
  }
  return (turns ?? []).some(
    (turn) =>
      turn.message_id === pendingInput.serverMessageId &&
      turn.output.some((item) => item.type === "user_input"),
  );
}

function createPendingInputBlock(
  streamState: StreamSessionState | undefined,
  turns: ChatApiTurns | undefined,
): ChatRenderBlock | undefined {
  const pendingInput = streamState?.pendingInput;
  if (!pendingInput?.text.trim()) {
    return undefined;
  }

  const isActivePendingInput = isStreamTurnActive(streamState);
  const hasCommittedInput = (turns ?? []).some(
    (turn) =>
      !pendingInput.serverMessageId &&
      !isActivePendingInput &&
      turn.output.some((item) => item.type === "user_input" && item.text === pendingInput.text),
  );

  if (hasCommittedInput) {
    return undefined;
  }

  return {
    id: `pending-user-${pendingInput.clientMessageId ?? streamState?.activeMessageId ?? "input"}`,
    kind: "user",
    text: pendingInput.text,
    live: true,
  };
}

function isStreamTurnActive(streamState: StreamSessionState | undefined) {
  if (streamState?.isStreaming) {
    return true;
  }
  return (
    streamState?.connectionState === "awaiting" ||
    streamState?.connectionState === "streaming" ||
    streamState?.connectionState === "detached" ||
    streamState?.connectionState === "reconnecting" ||
    streamState?.connectionState === "cancelling"
  );
}
