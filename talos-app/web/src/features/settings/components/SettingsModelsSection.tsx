import { RefreshCw } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Badge } from "@/shared/ui/Badge";
import { Panel } from "@/shared/ui/Panel";
import { FieldBlock } from "./SectionPrimitives";

export function SettingsModelsSection({
  defaultModel,
  setDefaultModel,
  models,
  modelCards,
}: Pick<SettingsPageState, "defaultModel" | "setDefaultModel" | "models" | "modelCards">) {
  const { t } = useTranslation("settings");

  return (
    <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
      <div className="mb-5 flex items-center justify-between">
        <div>
          <div className="text-sm font-medium text-ink">{t("modelConfiguration")}</div>
          <p className="mt-1 text-sm text-muted">{t("modelConfigurationDetail")}</p>
        </div>
        <RefreshCw className={models.isFetching ? "animate-spin text-brand" : "text-muted"} size={16} />
      </div>
      <FieldBlock label={t("defaultModel")}>
        <select
          className="w-full rounded-2xl border border-border bg-surface/85 px-4 py-3 text-sm outline-none transition focus:border-brand/50"
          onChange={(event) => setDefaultModel(event.target.value)}
          value={defaultModel}
        >
          {modelCards.map((model) => (
            <option key={model.id} value={model.id}>
              {model.name}
            </option>
          ))}
        </select>
      </FieldBlock>
      <div className="mt-5 space-y-3">
        {modelCards.map((model) => (
          <div className="rounded-[1.25rem] border border-border/80 bg-background/45 p-4" key={model.id}>
            <div className="flex items-center justify-between gap-3">
              <div className="font-medium text-ink">{model.name}</div>
              <Badge>{model.provider}</Badge>
            </div>
            <div className="mt-2 text-xs text-muted">
              {t("modelStats", {
                contextWindow: model.context_window ?? 0,
                tokenCount: model.max_tokens ?? 0,
              })}
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
