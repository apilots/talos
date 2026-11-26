import { useTranslation } from "react-i18next";
import type { FlowLayer } from "@/features/computers";

interface ComputerFlowFilterProps {
  layer: FlowLayer;
  onChange: (layer: FlowLayer) => void;
}

export function ComputerFlowFilter({ layer, onChange }: ComputerFlowFilterProps) {
  const { t } = useTranslation("computers");
  return (
    <div className="cmp-flow-buttons" aria-label={t("computers:flowFilter")}>
      {(["all", "execution", "sync"] as const).map((value) => (
        <button
          type="button"
          key={value}
          aria-pressed={layer === value}
          onClick={() => onChange(value)}
        >
          {value !== "all" ? (
            <i
              className={`cmp-flow-key ${value === "sync" ? "is-sync" : ""}`}
            />
          ) : null}
          {t(
            value === "all"
              ? "computers:allFlows"
              : value === "execution"
                ? "computers:executionFlow"
                : "computers:syncFlow",
          )}
        </button>
      ))}
    </div>
  );
}
