import { Check, CircleStop, LoaderCircle, TriangleAlert } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ContextCompactionOutcome } from "@/shared/types/protocol";
import { CommandResponseSurface } from "./CommandResponseSurface";
import type { ComposerCommandResponseState } from "./command-response.types";

type CompactionResponse = Extract<ComposerCommandResponseState, { kind: "compact" }>;

interface ComposerCompactionResponseProps {
  onDismiss?: () => void;
  response: CompactionResponse;
}

/** Presents user-requested compaction above the Composer without duplicating Timeline events. */
export function ComposerCompactionResponse({
  onDismiss,
  response,
}: ComposerCompactionResponseProps) {
  const { t } = useTranslation("chat");
  if (response.state === "running") {
    const stage = response.stage ?? "preparing";
    return (
      <CommandResponseSurface
        actions={(
          <span className="text-[11px] font-medium text-muted">
            {t(`commandCompactStageLabel.${stage}`)}
          </span>
        )}
        icon={<LoaderCircle aria-hidden="true" className="motion-safe:animate-spin" size={14} />}
        label={t("commandCompactProgressLabel")}
        title={t("commandCompactProgressTitle")}
      >
        <div className="px-4 py-3">
          <p className="text-[12px] leading-5 text-muted">
            {t(`commandCompactStageDetail.${stage}`)}
          </p>
          <div aria-hidden="true" className="relative mt-2.5 h-0.5 overflow-hidden rounded-full bg-surface">
            <span className="absolute inset-y-0 w-1/4 rounded-full bg-brand/75 motion-safe:animate-[command-progress_1.4s_ease-in-out_infinite]" />
          </div>
        </div>
      </CommandResponseSurface>
    );
  }

  const copy = terminalCopy(response.state, response.outcome, t);
  const failed = response.state === "error";
  return (
    <CommandResponseSurface
      className={failed ? "border-danger/30" : undefined}
      icon={failed
        ? <TriangleAlert aria-hidden="true" size={14} />
        : response.state === "cancelled"
          ? <CircleStop aria-hidden="true" size={14} />
          : <Check aria-hidden="true" size={14} />}
      label={copy.title}
      onDismiss={onDismiss}
      role={failed ? "alert" : "status"}
      title={copy.title}
    >
      <div className="px-4 py-3 text-[12px] leading-5 text-muted">
        {response.message ?? copy.detail}
      </div>
    </CommandResponseSurface>
  );
}

function terminalCopy(
  state: Exclude<CompactionResponse["state"], "running">,
  outcome: ContextCompactionOutcome | undefined,
  t: ReturnType<typeof useTranslation>["t"],
) {
  const key = state === "cancelled" ? "cancelled" : outcome ?? (state === "error" ? "failed" : "applied");
  return {
    detail: t(`commandCompactOutcome.${key}.detail`),
    title: t(`commandCompactOutcome.${key}.title`),
  };
}
