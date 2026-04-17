import { useTranslation } from "react-i18next";

import type { AdminPageState } from "../hooks/useAdminPage";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AdminModelsSection({ models }: Pick<AdminPageState, "models">) {
  const { t } = useTranslation("admin");

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("modelsPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("modelsPanel.detail")}</div>
        </div>
        <Button variant="secondary">{t("modelsPanel.add")}</Button>
      </div>
      <div className="space-y-3">
        {(models.data ?? []).map((model) => (
          <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4" key={model.id}>
            <div className="flex items-center justify-between gap-3">
              <div>
                <div className="font-medium text-ink">{model.name}</div>
                <div className="mt-1 text-xs text-muted">
                  {t("modelsPanel.provider", {
                    provider: model.provider,
                    contextWindow: model.context_window ?? 0,
                  })}
                </div>
              </div>
              <Badge tone="success">{t("states.healthy")}</Badge>
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
