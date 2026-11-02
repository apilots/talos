import { Select } from "@/shared/ui/Select";
import { useTranslation } from "react-i18next";
import type { TraceTurn } from "../model/selectors";
export function TraceToolbar({
  sessionId,
  turns,
  turn,
  range,
  onRange,
}: {
  sessionId: string;
  turns: TraceTurn[];
  turn?: TraceTurn;
  range: string;
  onRange: (range: string) => void;
}) {
  const { t } = useTranslation("tracing");
  return (
    <header className="tracing-heading">
      <div>
        <h1>{t("title")}</h1>
        <p>{t("description")}</p>
      </div>
      <div className="tracing-turn-control">
        <label htmlFor={`tracing-range-${sessionId}`}>{t("range")}</label>
        <Select
          id={`tracing-range-${sessionId}`}
          aria-label={t("range")}
          className="tracing-select"
          value={range}
          onValueChange={onRange}
          options={[
            { value: "all", label: t("allLoadedTurns") },
            ...turns.map((item) => ({
              value: item.identity.turnId,
              label: `Turn ${item.ordinal} · ${t(`states.${item.status}`)}`,
            })),
          ]}
        />
        <small>
          {t("selectedTurn")} {turn?.ordinal ?? "—"} ·{" "}
          {turn ? t(`states.${turn.status}`) : "—"}
        </small>
      </div>
    </header>
  );
}
