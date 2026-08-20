import { CircleAlert, CircleDot, Gauge, Repeat2, Wrench } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ChatTurnMetrics } from "@/shared/types/ui";
import { cn } from "@/shared/lib/cn";
import {
  calculateTurnTokensPerSecond,
  formatMetricNumber,
  formatTokensPerSecond,
} from "./turnMetrics";

export function TurnMetricsFooter({ metrics }: { metrics: ChatTurnMetrics }) {
  const { t } = useTranslation("chat");
  const tokensPerSecond = calculateTurnTokensPerSecond(
    metrics.usage.total_tokens,
    metrics.duration_ms,
  );
  const items = [
    tokensPerSecond === null
      ? null
      : {
          icon: Gauge,
          key: "tokensPerSecond",
          label: t("tokensPerSecond", { value: formatTokensPerSecond(tokensPerSecond) }),
        },
    {
      icon: CircleDot,
      key: "tokens",
      label: t("tokens", { value: formatMetricNumber(metrics.usage.total_tokens) }),
    },
    {
      icon: Repeat2,
      key: "rounds",
      label: t(metrics.model_rounds === 1 ? "round" : "rounds", { value: metrics.model_rounds }),
    },
    {
      icon: Wrench,
      key: "tools",
      label: t(metrics.tool_calls === 1 ? "tool" : "tools", { value: metrics.tool_calls }),
    },
    {
      icon: CircleAlert,
      key: "errors",
      label: t(metrics.tool_errors === 1 ? "error" : "errors", { value: metrics.tool_errors }),
    },
  ].filter((item): item is NonNullable<typeof item> => item !== null);

  return (
    <footer
      aria-label={t("turnAverageMetrics")}
      className="mt-4 flex flex-wrap items-center gap-x-2 gap-y-1.5 border-t border-border/45 pt-2.5 text-[11px] text-muted"
      data-turn-metrics="true"
    >
      {items.map(({ icon: Icon, key, label }, index) => (
        <span className="contents" key={key}>
          {index > 0 ? <span aria-hidden="true" className="text-border">·</span> : null}
          <span
            className={cn(
              "inline-flex items-center gap-1 whitespace-nowrap",
              key === "errors" && metrics.tool_errors > 0 && "text-danger",
            )}
            data-metric={key}
          >
            <Icon aria-hidden="true" size={12} strokeWidth={1.6} />
            {label}
          </span>
        </span>
      ))}
    </footer>
  );
}
