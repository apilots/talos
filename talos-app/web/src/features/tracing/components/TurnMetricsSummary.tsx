import { useTranslation } from "react-i18next";
import type { AppProtocol as API } from "@talos/sdk-core";
import { milliseconds, number } from "../model/formatters";
export function TurnMetricsSummary({
  metrics,
  ordinal,
}: {
  metrics?: API.AgentRunMetrics;
  ordinal: number;
}) {
  const { t, i18n } = useTranslation("tracing");
  if (!metrics)
    return (
      <div className="tracing-original-metrics">
        <strong>{t("originalTitle", { number: ordinal })}</strong>
        <span>{t("metricsMissing")}</span>
      </div>
    );
  const values = [
    [t("duration"), milliseconds(metrics.durationMs, i18n.language)],
    [t("modelRounds"), number(metrics.modelRounds, i18n.language)],
    [t("toolCalls"), number(metrics.toolCalls, i18n.language)],
    [t("toolSuccesses"), number(metrics.toolSuccesses, i18n.language)],
    [t("toolErrors"), number(metrics.toolErrors, i18n.language)],
    [t("input"), number(metrics.usage.input_tokens, i18n.language)],
    [t("output"), number(metrics.usage.output_tokens, i18n.language)],
    [t("total"), number(metrics.usage.total_tokens, i18n.language)],
    [
      t("cacheRead"),
      number(metrics.usage.cache_read_input_tokens, i18n.language),
    ],
    [
      t("cacheWrite"),
      number(metrics.usage.cache_creation_input_tokens, i18n.language),
    ],
  ];
  return (
    <div className="tracing-original-metrics">
      <strong>{t("originalTitle", { number: ordinal })}</strong>
      <dl className="flex flex-wrap gap-x-5 gap-y-1">
        {values.map(([label, value]) => (
          <div key={label} className="flex gap-2">
            <dt className="text-muted">{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      <small className="tracing-summary-scope">{t("originalScope")}</small>
      {metrics.context ? (
        <details className="tracing-context-metrics">
          <summary>{t("contextMetrics")}</summary>
          <dl className="flex flex-wrap">
            {[
              [
                t("contextUsed"),
                number(metrics.context.usedTokens, i18n.language),
              ],
              [
                t("contextWindow"),
                number(metrics.context.contextWindowTokens, i18n.language),
              ],
              [
                t("promptBudget"),
                number(metrics.context.effectivePromptBudgetTokens, i18n.language),
              ],
              [
                t("measuredAt"),
                new Date(metrics.context.measuredAt * 1000).toLocaleString(i18n.language),
              ],
            ].map(([label, value]) => (
              <div className="flex" key={label}>
                <dt>{label}</dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
        </details>
      ) : null}
    </div>
  );
}
