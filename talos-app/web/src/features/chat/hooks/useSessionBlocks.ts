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
  stoppingResponseDetail,
  stoppingResponseTitle,
}: {
  sessionId: string;
  streamState?: StreamSessionState;
  turns?: ChatApiTurns;
  waitingResponseDetail: string;
  waitingResponseTitle: string;
  stoppingResponseDetail: string;
  stoppingResponseTitle: string;
}) {
  const historyBlocks = mapTurnsToBlocks(turns ?? []);
  const pendingInputBlock = createPendingInputBlock(streamState, turns);
  const liveBlocks = streamState?.blocks ?? [];
  const shouldShowLiveBlocks =
    Boolean(pendingInputBlock) ||
    liveBlocks.length > 0 ||
    (streamState?.isStreaming ?? false) ||
    Boolean(streamState?.error) ||
    Boolean(streamState?.pendingUserAction) ||
    Boolean(streamState?.pendingApproval);
  const nextBlocks = [
    ...historyBlocks,
    ...(pendingInputBlock ? [pendingInputBlock] : []),
    ...(shouldShowLiveBlocks ? liveBlocks : []),
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

function createPendingInputBlock(
  streamState: StreamSessionState | undefined,
  turns: ChatApiTurns | undefined,
): ChatRenderBlock | undefined {
  const pendingInput = streamState?.pendingInput;
  if (!pendingInput?.text.trim()) {
    return undefined;
  }

  const isActivePendingInput = isStreamTurnActive(streamState);
  const hasCommittedInput = (turns ?? []).some((turn) => {
    const hasMatchingInput = turn.output.some(
      (item) => item.type === "user_input" && item.text === pendingInput.text,
    );
    if (!hasMatchingInput) {
      return false;
    }

    if (pendingInput.serverMessageId) {
      return turn.message_id === pendingInput.serverMessageId || !isActivePendingInput;
    }

    return !isActivePendingInput;
  });

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
