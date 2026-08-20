import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { TalosContentItem, TalosToolProgressKind } from "@/shared/types/protocol";
import type { ChatToolStatus } from "@/shared/types/ui";
import { ToolCallCard } from "@/widgets/tool-call-card/ToolCallCard";
import { formatDuration } from "./formatDuration";
import { buildToolDisplayTitle } from "./toolDisplay";
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

function toolStatusLabel(status: ChatToolStatus) {
  switch (status) {
    case "queued":
      return "Queued";
    case "running":
      return "Running";
    case "waiting_dynamic_input":
      return "Waiting";
    case "done":
      return "Completed";
    case "error":
      return "Failed";
    case "cancelled":
      return "Cancelled";
  }
}

export function ToolCallBlock({
  argumentsText,
  callId,
  contentItems,
  output,
  progress,
  prompt,
  status,
  timingMs,
  toolName,
  variant = "default",
}: {
  argumentsText: string;
  callId: string;
  contentItems?: TalosContentItem[];
  output?: string;
  progress: Array<{ kind: TalosToolProgressKind; message: string }>;
  prompt?: string;
  status: ChatToolStatus;
  timingMs?: number | null;
  toolName: string;
  variant?: "default" | "timeline";
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(status === "waiting_dynamic_input");
  const title = buildToolDisplayTitle(toolName, argumentsText);
  const resultDurationMs = contentItems
    ?.filter((item) => item.type === "bash_output")
    .filter((item) => item.elapsed_ms !== null)
    .at(-1)?.elapsed_ms;
  const durationMs = timingMs ?? resultDurationMs;
  const durationLabel = durationMs !== null && durationMs !== undefined ? formatDuration(durationMs) : undefined;
  const compact = variant === "timeline";

  useEffect(() => {
    if (status === "waiting_dynamic_input") {
      setOpen(true);
    }
  }, [status]);

  return (
    <ToolCallCard.Root compact={compact}>
      <ToolCallCard.Header
        compact={compact}
        onToggle={() => setOpen((value) => !value)}
        open={open}
        durationLabel={durationLabel}
        statusLabel={toolStatusLabel(status)}
        title={title}
        tone={resolveTone(status)}
      />
      {open ? (
        <ToolCallCard.Body compact={compact}>
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
            <ToolCallCard.Input content={argumentsText} title={t("arguments")} />
            {progress.length > 0 ? <ToolCallCard.Progress entries={progress} title={t("progress")} /> : null}
          </ToolCallCard.Details>
        </ToolCallCard.Body>
      ) : (
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
