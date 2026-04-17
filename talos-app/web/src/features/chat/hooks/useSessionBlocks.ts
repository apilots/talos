import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { mapTurnsToBlocks } from "@/shared/types/domain";
import type { StreamSessionState } from "@/shared/types/ui";
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
    const historyBlocks = mapTurnsToBlocks(turns ?? []);
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
        title: t("waitingResponseTitle"),
        detail: t("waitingResponseDetail"),
        live: true,
      });
    }

    if (streamState?.connectionState === "cancelling") {
      nextBlocks.push({
        id: `status-cancelling-${sessionId}`,
        kind: "status",
        variant: "cancelling",
        title: t("stoppingResponseTitle"),
        detail: t("stoppingResponseDetail"),
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
    t,
    turns,
  ]);
}
