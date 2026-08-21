import { Check, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ContextUsage } from "../contextUsage";
import { ComposerCompactionResponse } from "./ComposerCompactionResponse";
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
  if (response.kind === "compact") {
    return <ComposerCompactionResponse onDismiss={onDismiss} response={response} />;
  }
  return <CommandResult onDismiss={onDismiss} response={response} />;
}

type ResultResponse = Exclude<
  Extract<ComposerCommandResponseState, { state: "success" | "error" }>,
  { kind: "compact" }
>;

function CommandResult({ onDismiss, response }: { onDismiss?: () => void; response: ResultResponse }) {
  const { t } = useTranslation(["chat", "common"]);
  const failed = response.state === "error";
  let title: string;
  let detail: string;

  if (response.kind === "clear") {
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
