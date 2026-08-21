import { Box, Check, LoaderCircle, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ContextUsage } from "../contextUsage";
import { CommandResponseSurface } from "./CommandResponseSurface";
import { ComposerStatusResponse } from "./ComposerStatusResponse";
import type {
  ComposerCommandResponseState,
  ComposerStatusSnapshot,
} from "./command-response.types";

interface ComposerCommandResponseProps {
  contextUsage?: ContextUsage;
  onDismiss?: () => void;
  response?: ComposerCommandResponseState;
  sessionTotalTokens?: number;
  status?: ComposerStatusSnapshot;
}

/** Routes semantic command state to focused response components above the Composer. */
export function ComposerCommandResponse({
  contextUsage,
  onDismiss,
  response,
  sessionTotalTokens,
  status,
}: ComposerCommandResponseProps) {
  if (!response) {
    return null;
  }
  if (response.kind === "status") {
    return status ? (
      <ComposerStatusResponse
        contextUsage={contextUsage}
        onDismiss={onDismiss}
        sessionTotalTokens={sessionTotalTokens}
        status={status}
      />
    ) : null;
  }
  if (response.kind === "compact" && response.state === "running") {
    return <CompactionProgress stage={response.stage} />;
  }
  return <CommandResult onDismiss={onDismiss} response={response} />;
}

type ResultResponse = Extract<ComposerCommandResponseState, { state: "success" | "error" }>;

function CommandResult({ onDismiss, response }: { onDismiss?: () => void; response: ResultResponse }) {
  const { t } = useTranslation(["chat", "common"]);
  const failed = response.state === "error";
  let title: string;
  let detail: string;

  if (response.kind === "compact") {
    title = t(`chat:commandCompact${failed ? "Error" : "Success"}Title`);
    detail = t(`chat:commandCompact${failed ? "Error" : "Success"}Detail`);
  } else if (response.kind === "clear") {
    title = failed ? t("chat:clearSessionFailed") : t("chat:commandClearSuccessTitle");
    detail = failed ? title : t("chat:commandClearSuccessDetail");
  } else {
    title = t(`chat:commandModel${failed ? "Error" : "Success"}Title`);
    detail = failed ? title : formatModelDetail(response, t);
  }

  return (
    <CommandResponseSurface
      className={failed ? "border-danger/30" : undefined}
      icon={failed
        ? <TriangleAlert aria-hidden="true" size={14} />
        : response.kind === "compact"
          ? <Box aria-hidden="true" size={14} />
          : <Check aria-hidden="true" size={14} />}
      label={title}
      onDismiss={onDismiss}
      role={failed ? "alert" : "status"}
      title={title}
    >
      <div className="px-4 py-3 text-[12px] text-muted">{response.message ?? detail}</div>
    </CommandResponseSurface>
  );
}

function CompactionProgress({ stage = "preparing" }: {
  stage?: Extract<ComposerCommandResponseState, { kind: "compact"; state: "running" }>["stage"];
}) {
  const { t } = useTranslation("chat");
  return (
    <CommandResponseSurface
      icon={<LoaderCircle aria-hidden="true" className="animate-spin" size={14} />}
      label={t("commandCompactProgressLabel")}
      title={t("commandCompactProgressTitle")}
    >
      <div className="px-4 py-3.5">
        <div className="mb-2.5 flex flex-wrap items-center justify-between gap-2 text-[12px]">
          <span className="font-medium text-ink/90">{t(`commandCompactStage.${stage}`)}</span>
          <span className="text-muted">{t("commandCompactPreserving")}</span>
        </div>
        <div aria-label={t("commandCompactProgressLabel")} className="relative h-1 overflow-hidden rounded-full bg-surface" role="progressbar">
          <span className="absolute inset-y-0 w-1/3 animate-[command-progress_1.2s_ease-in-out_infinite] rounded-full bg-brand" />
        </div>
      </div>
    </CommandResponseSurface>
  );
}

function formatModelDetail(
  response: Extract<ComposerCommandResponseState, { kind: "model" }>,
  t: ReturnType<typeof useTranslation>["t"],
) {
  if (!response.reasoningLevel || response.reasoningLevel === "none") {
    return response.model;
  }
  const level = `${response.reasoningLevel.charAt(0).toUpperCase()}${response.reasoningLevel.slice(1)}`
    .replace("_high", " High");
  return `${response.model} · ${t(`chat:reasoning${level.replace(" ", "")}`)}`;
}
