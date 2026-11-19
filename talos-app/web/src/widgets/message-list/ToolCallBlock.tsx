import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { useNowMs } from "@/shared/hooks/useNowMs";
import type { TalosContentItem, TalosToolProgressKind } from "@/shared/types/protocol";
import type { ChatToolStatus } from "@/shared/types/ui";
import { ToolCallCard } from "@/widgets/tool-call-card/ToolCallCard";
import { formatDuration } from "@/shared/lib/formatDuration";
import { timelineToolTitle } from "./timelineToolPresentation";
import { buildToolDisplayTitle, toolCommand, toolSearchQuery } from "./toolDisplay";
import { ToolResultPreview, ToolResultRenderer } from "./ToolResultRenderer";

function resolveTone(status: ChatToolStatus) {
  if (status === "done") {
    return "success";
  }
  if (status === "waiting_dynamic_input") {
    return "warning";
  }
  if (status === "error") {
    return "danger";
  }
  return "default";
}

function RunningToolDuration({ startedAtMs }: { startedAtMs: number }) {
  const elapsedSeconds = Math.max(0, Math.floor((useNowMs(true) - startedAtMs) / 1000));
  return elapsedSeconds === 0 ? "0s" : formatDuration(elapsedSeconds * 1000);
}

export function ToolCallBlock({
  argumentsValue,
  callId,
  contentItems,
  live = true,
  output,
  progress,
  prompt,
  startedAtMs,
  status,
  timingMs,
  toolName,
  variant = "default",
}: {
  argumentsValue: unknown;
  callId: string;
  contentItems?: TalosContentItem[];
  live?: boolean;
  output?: string;
  progress: Array<{ kind: TalosToolProgressKind; message: string }>;
  prompt?: string;
  startedAtMs?: number;
  status: ChatToolStatus;
  timingMs?: number | null;
  toolName: string;
  variant?: "default" | "timeline" | "activity";
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(status === "waiting_dynamic_input");
  const compact = variant !== "default";
  const collapsedActivity = variant === "activity" && !open;
  const isToolSearch = toolName === "ToolSearch";
  const query = isToolSearch ? toolSearchQuery(argumentsValue) : undefined;
  const title = isToolSearch ? toolName
    : variant === "activity" ? timelineToolTitle(toolName, argumentsValue, t)
      : buildToolDisplayTitle(toolName, argumentsValue);
  const command = open && compact && toolName === "Bash" ? toolCommand(argumentsValue) : undefined;
  const resultDurationMs = contentItems
    ?.filter((item) => item.type === "bash_output")
    .filter((item) => item.elapsed_ms !== null)
    .at(-1)?.elapsed_ms;
  const durationMs = timingMs ?? resultDurationMs;
  const running = live && status === "running";
  const statusKey = !compact && status === "waiting_dynamic_input" ? "waiting" : status;
  const duration = running && startedAtMs !== undefined && startedAtMs > 0
    ? <RunningToolDuration startedAtMs={startedAtMs} />
    : durationMs != null ? formatDuration(durationMs) : undefined;
  const showQuietCommand = compact && running && toolName === "Bash" &&
    !output?.trim() && !contentItems?.length &&
    !progress.some(({ kind, message }) => kind !== "status" && message.trim());

  useEffect(() => {
    if (status === "waiting_dynamic_input") {
      setOpen(true);
    }
  }, [status]);

  return (
    <ToolCallCard.Root compact={compact}>
      <ToolCallCard.Header
        compact={compact}
        description={query ? `· ("${query}")` : undefined}
        onToggle={() => setOpen((value) => !value)}
        open={open}
        duration={collapsedActivity ? undefined : duration}
        running={running}
        statusLabel={collapsedActivity ? undefined :
          status === "running" && !live ? t("toolLastSeenRunning") : t(`toolStatus.${statusKey}`)}
        title={title}
        tone={resolveTone(status)}
      />
      {showQuietCommand ? <p className="px-1 text-xs leading-5 text-muted">{t("toolNoOutputYet")}</p> : null}
      {open ? (
        <ToolCallCard.Body compact={compact}>
          {command ? <ToolCallCard.Input content={command} title={t("toolCommand")} /> : null}
          <ToolResultRenderer
            compact={compact}
            contentItems={contentItems}
            fallbackOutput={output}
            progress={progress}
            status={status}
          />
          {prompt ? <ToolCallCard.Prompt prompt={prompt} title={t("dynamicToolPrompt")} /> : null}
          <ToolCallCard.Details compact={compact} title={t("details")}>
            <ToolCallCard.MetaLine label="call_id" value={callId} />
            <ToolCallCard.Input
              content={typeof argumentsValue === "string" ? argumentsValue : JSON.stringify(argumentsValue, null, 2)}
              title={t("arguments")}
            />
            {progress.length > 0 ? <ToolCallCard.Progress entries={progress} title={t("progress")} /> : null}
          </ToolCallCard.Details>
        </ToolCallCard.Body>
      ) : collapsedActivity ? null : (
        <ToolResultPreview
          compact={compact}
          contentItems={contentItems}
          fallbackOutput={output}
          progress={progress}
          status={status}
        />
      )}
    </ToolCallCard.Root>
  );
}
