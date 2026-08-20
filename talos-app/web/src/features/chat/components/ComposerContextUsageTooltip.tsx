import { useId, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { formatRelativeTime } from "@/shared/lib/time";
import { contextRemainingTokens, type ContextUsage } from "./contextUsage";

interface ComposerContextUsageTooltipProps {
  children: ReactNode;
  label: string;
  percent: string;
  sessionTotalTokens?: number;
  usage: ContextUsage;
}

export function ComposerContextUsageTooltip({
  children,
  label,
  percent,
  sessionTotalTokens,
  usage,
}: ComposerContextUsageTooltipProps) {
  const { t } = useTranslation("chat");
  const tooltipId = useId();
  const rows = [
    {
      label: t("contextUsageTokens"),
      value: t("contextUsageTokenRatio", {
        used: formatTokens(usage.usedTokens),
        total: formatTokens(usage.contextWindowTokens),
      }),
    },
    {
      label: t("contextUsageRemaining"),
      value: t("contextUsageTokenCount", {
        value: formatTokens(contextRemainingTokens(usage)),
      }),
    },
    ...(sessionTotalTokens === undefined
      ? []
      : [
          {
            label: t("contextUsageSessionTotal"),
            value: t("contextUsageTokenCount", { value: formatTokens(sessionTotalTokens) }),
          },
        ]),
    {
      label: t("contextUsagePromptBudget"),
      value: t("contextUsageTokenCount", {
        value: formatTokens(usage.effectivePromptBudgetTokens),
      }),
    },
    {
      label: t("contextUsageUpdated"),
      value: formatRelativeTime(usage.measuredAt * 1000),
    },
  ];

  return (
    <span className="group relative shrink-0">
      <span
        aria-describedby={tooltipId}
        aria-label={label}
        className="flex h-8 items-center gap-1.5 rounded-md px-1 text-[12px] tabular-nums text-muted outline-none transition-colors hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/25"
        data-testid="composer-context-usage"
        role="img"
        tabIndex={0}
      >
        {children}
      </span>

      <span
        className="pointer-events-none absolute bottom-[calc(100%+8px)] right-0 z-50 w-[248px] translate-y-1 rounded-xl border border-border/70 bg-card p-3 text-left opacity-0 shadow-[0_14px_36px_rgba(0,0,0,0.14)] transition duration-150 group-hover:translate-y-0 group-hover:opacity-100 group-focus-within:translate-y-0 group-focus-within:opacity-100"
        id={tooltipId}
        role="tooltip"
      >
        <span className="mb-2.5 flex items-center justify-between gap-3">
          <span className="text-[12px] font-medium text-ink">{t("contextUsageTitle")}</span>
          <span className="text-[12px] font-semibold tabular-nums text-success">
            {percent}%
          </span>
        </span>
        <span className="grid gap-1.5">
          {rows.map((row) => (
            <span
              className="grid grid-cols-[minmax(0,1fr)_max-content] items-baseline gap-3 text-[11px]"
              key={row.label}
            >
              <span className="text-muted">{row.label}</span>
              <span className="font-medium tabular-nums text-ink/85">{row.value}</span>
            </span>
          ))}
        </span>
      </span>
    </span>
  );
}

function formatTokens(value: number) {
  return new Intl.NumberFormat("en-US", {
    maximumFractionDigits: 1,
    notation: "compact",
  }).format(value);
}
