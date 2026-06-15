import { useState } from "react";
import { useTranslation } from "react-i18next";

import type { TalosContentItem, TalosToolProgressKind } from "@/shared/types/protocol";
import { ToolCallCard } from "@/widgets/tool-call-card/ToolCallCard";
import { formatDuration } from "./formatDuration";
import { ToolResultPreview, ToolResultRenderer, ToolResultSummary } from "./ToolResultRenderer";

function resolveTone(status: string) {
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
  status: "queued" | "running" | "waiting_dynamic_input" | "done" | "error";
  timingMs?: number | null;
  toolName: string;
  variant?: "default" | "timeline";
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(status === "waiting_dynamic_input");
  const resultSummary = ToolResultSummary({
    contentItems,
    fallbackOutput: output,
    progress,
    status,
  });

  return (
    <ToolCallCard.Root compact={variant === "timeline"}>
      <ToolCallCard.Header
        onToggle={() => setOpen((value) => !value)}
        open={open}
        statusLabel={resultSummary || status.replaceAll("_", " ")}
        title={toolName}
        tone={resolveTone(status)}
      />
      {open ? (
        <ToolCallCard.Body>
          <ToolResultRenderer
            contentItems={contentItems}
            fallbackOutput={output}
            progress={progress}
            status={status}
          />
          {prompt ? <ToolCallCard.Prompt prompt={prompt} title={t("dynamicToolPrompt")} /> : null}
          <ToolCallCard.Details title={t("details")}>
            <ToolCallCard.MetaLine label="call_id" value={callId} />
            <ToolCallCard.Input content={argumentsText} title={t("arguments")} />
            {progress.length > 0 ? <ToolCallCard.Progress entries={progress} title={t("progress")} /> : null}
          </ToolCallCard.Details>
          {timingMs !== null && timingMs !== undefined ? (
            <div className="text-[11px] text-muted">{t("duration", { value: formatDuration(timingMs) })}</div>
          ) : null}
        </ToolCallCard.Body>
      ) : (
        <ToolResultPreview
          contentItems={contentItems}
          fallbackOutput={output}
          progress={progress}
          status={status}
        />
      )}
    </ToolCallCard.Root>
  );
}
