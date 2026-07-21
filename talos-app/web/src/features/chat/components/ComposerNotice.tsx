import { useTranslation } from "react-i18next";

import { contextCompactionStageLabel } from "../timeline-state";
import type { ContextCompactionStage } from "@/shared/types/protocol";

interface ComposerNoticeProps {
  commandNotice?: string;
  contextCompactionStage?: ContextCompactionStage;
}

export function ComposerNotice({ commandNotice, contextCompactionStage }: ComposerNoticeProps) {
  const { t } = useTranslation("chat");
  const notice = contextCompactionStage
    ? t("contextCompacting", { stage: contextCompactionStageLabel(contextCompactionStage) })
    : commandNotice;

  return notice ? (
    <div className="mb-2 px-3 text-xs font-medium text-muted" role="status">
      {notice}
    </div>
  ) : null;
}
