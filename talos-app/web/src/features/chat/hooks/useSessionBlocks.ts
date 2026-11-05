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

  const history = useMemo(() => buildBlocksFromHistorySnapshot(sessionHistory), [sessionHistory]);
  return useMemo(() => {
    return buildSessionBlocks({
      sessionId,
      streamState,
      history,
      waitingResponseDetail: t("waitingResponseDetail"),
      waitingResponseTitle: t("waitingResponseTitle"),
      stoppingResponseDetail: t("stoppingResponseDetail"),
      stoppingResponseTitle: t("stoppingResponseTitle"),
    });
  }, [
    sessionId,
    streamState,
    history,
    t,
  ]);
}

export function buildSessionBlocks({
  sessionId,
  streamState,
  history,
  waitingResponseDetail,
  waitingResponseTitle,
  stoppingResponseDetail,
  stoppingResponseTitle,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  history: ReturnType<typeof buildBlocksFromHistorySnapshot>;
  waitingResponseDetail: string;
  waitingResponseTitle: string;
  stoppingResponseDetail: string;
  stoppingResponseTitle: string;
}) {
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
  const historyNoticeIds = new Set(historyBlocks.filter((block) => block.kind === "notice").map((block) => block.id));
  const historyById = new Map(historyBlocks.map((block) => [block.id, block]));
  const liveBlocks = (streamState?.blocks ?? []).map((block) => {
    if (block.kind !== "user") return block;
    const historical = historyById.get(block.id);
    if (historical?.kind !== "user" || ((block.automationOrigin || !historical.automationOrigin) && (block.activationMode || !historical.activationMode))) return block;
    return { ...block,
      ...(!block.automationOrigin && historical.automationOrigin ? { automationOrigin: historical.automationOrigin } : {}),
      ...(!block.activationMode && historical.activationMode ? { activationMode: historical.activationMode } : {}),
    };
  }).filter((block) =>
    block.kind === "notice"
      ? !historyNoticeIds.has(block.id)
      : !historyHasActiveMessage || Boolean(streamState?.isStreaming),
  );
  const liveBlockIds = new Set(liveBlocks.map((block) => block.id));
  const liveHasPendingInput = Boolean(
    streamState?.pendingInput?.serverMessageId &&
      liveBlocks.some(
        (block) =>
          block.kind === "user" &&
          block.id.endsWith(`-${streamState.pendingInput?.serverMessageId}`),
      ),
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
    id: `user-${pendingInput.serverMessageId ?? pendingInput.clientMessageId}`,
    kind: "user",
    ...(pendingInput.activationMode ? { activationMode: pendingInput.activationMode } : {}),
    text: pendingInput.text,
    fileIds: pendingInput.fileIds,
    live: true,
  };
}
