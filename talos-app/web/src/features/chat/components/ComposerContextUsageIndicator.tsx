import type { SessionInfo } from "@talos/sdk-browser";

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
  const percent = usage ? contextUsedPercent(usage) : 0;
  if (!usage || percent === 0) {
    return null;
  }
  const roundedPercent = Number(percent.toFixed(1));
  const formattedPercent = roundedPercent.toFixed(1);
  const label = `Context usage: ${formattedPercent}%`;

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
      <span>{formattedPercent}%</span>
    </ComposerContextUsageTooltip>
  );
}
