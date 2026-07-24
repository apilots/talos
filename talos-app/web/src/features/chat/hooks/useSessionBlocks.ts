import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import type { ChatRenderBlock, StreamSessionState } from "@/shared/types/ui";
import { buildBlocksFromHistorySnapshot } from "../stream-events";
import type { ChatApiSessionHistory } from "../api";

export function useSessionBlocks({
  sessionId,
  streamState,
  sessionHistory,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  sessionHistory?: ChatApiSessionHistory;
}) {
  const { t } = useTranslation("chat");

  return useMemo(() => {
    return buildSessionBlocks({
      sessionId,
      streamState,
      sessionHistory,
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
    sessionHistory,
    t,
  ]);
}

export function buildSessionBlocks({
  sessionId,
  streamState,
  sessionHistory,
  waitingResponseDetail,
  waitingResponseTitle,
  finalResponseWaitDetail = "Talos is composing the final response.",
  finalResponseWaitTitle = "Preparing final response",
  stoppingResponseDetail,
  stoppingResponseTitle,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  sessionHistory?: ChatApiSessionHistory;
  waitingResponseDetail: string;
  waitingResponseTitle: string;
  finalResponseWaitDetail?: string;
  finalResponseWaitTitle?: string;
  stoppingResponseDetail: string;
  stoppingResponseTitle: string;
}) {
  const history = buildBlocksFromHistorySnapshot(sessionHistory);
  const recoveredMessageIds = history.messageIds;
  const recoveredInputItems = history.inputItems;
  const historyBlocks = history.blocks;
  const historyHasActiveMessage = hasHistoryMessage(
    recoveredMessageIds,
    streamState?.activeMessageId,
  );
  const historyHasPendingInput = hasHistoryPendingInput(
    recoveredInputItems,
    streamState?.pendingInput,
  );
  const liveBlocks =
    historyHasActiveMessage && !streamState?.isStreaming ? [] : streamState?.blocks ?? [];
  const liveBlockIds = new Set(liveBlocks.map((block) => block.id));
  const liveHasPendingInput = Boolean(
    streamState?.pendingInput?.serverMessageId &&
      liveBlockIds.has(`user-${streamState.pendingInput.serverMessageId}`),
  );
  const pendingInputBlock =
    historyHasPendingInput || liveHasPendingInput ? undefined : createPendingInputBlock(streamState);
  const nextBlocks = [
    ...historyBlocks.filter((block) => !liveBlockIds.has(block.id)),
    ...(pendingInputBlock ? [pendingInputBlock] : []),
    ...liveBlocks,
  ];
  const hasLiveResponse = liveBlocks.some(
    (block) =>
      block.kind === "assistant" ||
      block.kind === "thinking" ||
      block.kind === "system" ||
      block.kind === "tool" ||
      block.kind === "timeline",
  );
  const shouldShowWaitingPlaceholder =
    Boolean(streamState?.isStreaming) &&
    !streamState?.pendingUserAction &&
    !streamState?.error &&
    !hasLiveResponse &&
    streamState?.connectionState !== "cancelling";

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

function hasHistoryMessage(messageIds: Set<string>, messageId: string | undefined) {
  return Boolean(messageId && messageIds.has(messageId));
}

function hasHistoryPendingInput(
  inputItems: Array<{ messageId: string }>,
  pendingInput: StreamSessionState["pendingInput"],
) {
  if (!pendingInput?.serverMessageId) {
    return false;
  }
  return inputItems.some((item) => item.messageId === pendingInput.serverMessageId);
}

function createPendingInputBlock(
  streamState: StreamSessionState | undefined,
): ChatRenderBlock | undefined {
  const pendingInput = streamState?.pendingInput;
  if (
    !pendingInput ||
    (!pendingInput.text.trim() && pendingInput.fileIds.length === 0) ||
    pendingInput.phase === "completed"
  ) {
    return undefined;
  }

  return {
    id: `pending-user-${pendingInput.clientMessageId}`,
    kind: "user",
    text: pendingInput.text,
    fileIds: pendingInput.fileIds,
    live: true,
  };
}
