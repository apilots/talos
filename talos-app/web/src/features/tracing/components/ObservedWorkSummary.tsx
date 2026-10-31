import type { AppProtocol as API } from "@talos/sdk-core";
import { useTranslation } from "react-i18next";
import { observedTotals, type TraceTurn } from "../model/selectors";
import { milliseconds, number } from "../model/formatters";
export function ObservedWorkSummary({
  entries,
  ordinal,
  turn,
}: {
  entries: API.TrajectoryEntry[];
  ordinal: number;
  turn: TraceTurn;
}) {
  const { t, i18n } = useTranslation("tracing");
  if (!entries.length) return null;
  const totals = observedTotals(entries, turn),
    partial = totals.partial;
  return (
    <div className="tracing-original-metrics tracing-observed-metrics">
      <strong>
        {t("pageScope", { number: ordinal })}
        {partial ? ` · ${t("loadedSubset")}` : ""}
      </strong>
      <dl className="flex flex-wrap gap-x-5 gap-y-1">
        {[
          [t("requests"), number(totals.requests, i18n.language)],
          [t("llm"), milliseconds(totals.llmMs, i18n.language)],
          [t("tools"), milliseconds(totals.toolMs, i18n.language)],
          [t("tokens"), number(totals.tokens, i18n.language)],
        ].map(([label, value]) => (
          <div key={label} className="flex gap-2">
            <dt>{label}</dt>
            <dd>{value}</dd>
          </div>
        ))}
      </dl>
      <small className="tracing-summary-scope">{t("observedScope")}</small>
    </div>
  );
}
