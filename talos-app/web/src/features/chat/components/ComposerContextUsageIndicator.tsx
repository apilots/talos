import type { SessionInfo } from "@talos/sdk-browser";
import { useTranslation } from "react-i18next";

import { ComposerContextUsageTooltip } from "./ComposerContextUsageTooltip";
import { contextUsedPercent } from "./contextUsage";

interface ComposerContextUsageIndicatorProps {
  sessionTotalTokens?: number;
  usage?: SessionInfo["usage"]["context"];
}

export function ComposerContextUsageIndicator({
  sessionTotalTokens,
  usage,
}: ComposerContextUsageIndicatorProps) {
  const { t } = useTranslation("chat");
  const percent = usage ? contextUsedPercent(usage) : 0;
  const roundedPercent = Number(percent.toFixed(1));
  const formattedPercent = usage ? roundedPercent.toFixed(1) : undefined;
  const label = `${t("contextUsageTitle")}: ${formattedPercent === undefined ? t("contextUsagePending") : `${formattedPercent}%`}`;

  return (
    <ComposerContextUsageTooltip
      label={label}
      percent={formattedPercent}
      sessionTotalTokens={sessionTotalTokens}
      usage={usage}
    >
      <svg aria-hidden="true" className="h-[18px] w-[18px] -rotate-90" viewBox="0 0 20 20">
        <circle
          className="text-success/20"
          cx="10"
          cy="10"
          fill="none"
          pathLength="100"
          r="7.5"
          stroke="currentColor"
          strokeWidth="2.5"
        />
        <circle
          className="text-success transition-[stroke-dasharray] duration-300"
          cx="10"
          cy="10"
          fill="none"
          pathLength="100"
          r="7.5"
          stroke="currentColor"
          strokeDasharray={`${roundedPercent} 100`}
          strokeLinecap="round"
          strokeWidth="2.5"
        />
      </svg>
      <span>{formattedPercent === undefined ? "—" : `${formattedPercent}%`}</span>
    </ComposerContextUsageTooltip>
  );
}
