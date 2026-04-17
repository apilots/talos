import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AdminToolsSection() {
  const { t } = useTranslation("admin");
  const toolRows = useMemo(
    () => [
      {
        id: "t-1",
        name: t("toolRows.webSearchName"),
        policy: t("states.allowed"),
        detail: t("toolRows.webSearchDetail"),
      },
      {
        id: "t-2",
        name: t("toolRows.readFilesName"),
        policy: t("states.allowed"),
        detail: t("toolRows.readFilesDetail"),
      },
      {
        id: "t-3",
        name: t("toolRows.writeFilesName"),
        policy: t("states.needsApproval"),
        detail: t("toolRows.writeFilesDetail"),
      },
      {
        id: "t-4",
        name: t("toolRows.terminalName"),
        policy: t("states.needsApproval"),
        detail: t("toolRows.terminalDetail"),
      },
      {
        id: "t-5",
        name: t("toolRows.externalHttpName"),
        policy: t("states.allowed"),
        detail: t("toolRows.externalHttpDetail"),
      },
    ],
    [t],
  );

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("toolsPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("toolsPanel.detail")}</div>
        </div>
        <Button variant="secondary">{t("toolsPanel.editPolicy")}</Button>
      </div>
      <div className="space-y-3">
        {toolRows.map((tool) => (
          <div
            className="grid grid-cols-[minmax(0,1fr)_180px] gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
            key={tool.id}
          >
            <div>
              <div className="text-sm font-medium text-ink">{tool.name}</div>
              <div className="mt-1 text-sm text-muted">{tool.detail}</div>
            </div>
            <div className="flex items-center justify-end">
              <Badge tone={tool.policy === t("states.allowed") ? "success" : "warning"}>
                {tool.policy}
              </Badge>
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
