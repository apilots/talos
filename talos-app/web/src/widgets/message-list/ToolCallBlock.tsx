import { useState } from "react";
import { useTranslation } from "react-i18next";

import { ToolCallCard } from "@/widgets/tool-call-card/ToolCallCard";

function resolveTone(status: string) {
  if (status === "done") {
    return "success";
  }
  if (status === "waiting_approval") {
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
  output?: string;
  progress: Array<{ kind: string; message: string }>;
  prompt?: string;
  status: "queued" | "running" | "waiting_approval" | "done" | "error";
  timingMs?: number | null;
  toolName: string;
  variant?: "default" | "timeline";
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(status === "waiting_approval");

  return (
    <ToolCallCard.Root compact={variant === "timeline"}>
      <ToolCallCard.Header
        callId={callId}
        onToggle={() => setOpen((value) => !value)}
        open={open}
        statusLabel={status.replaceAll("_", " ")}
        title={toolName}
        tone={resolveTone(status)}
      />
      {open ? (
        <ToolCallCard.Body>
          <ToolCallCard.Input content={argumentsText} title={t("arguments")} />
          {prompt ? <ToolCallCard.ApprovalActions prompt={prompt} title={t("approvalPrompt")} /> : null}
          {progress.length > 0 ? <ToolCallCard.Progress entries={progress} title={t("progress")} /> : null}
          {output ? <ToolCallCard.Output content={output} title={t("output")} /> : null}
          {timingMs ? <div className="text-[11px] text-muted">{t("duration", { value: timingMs })}</div> : null}
        </ToolCallCard.Body>
      ) : null}
    </ToolCallCard.Root>
  );
}
