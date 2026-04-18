import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Panel } from "@/shared/ui/Panel";

export function AdminAuditSection() {
  const { t } = useTranslation("admin");
  const auditRows = useMemo(
    () => [
      {
        id: "a-1",
        time: "14:32:11",
        actor: "xujiang",
        event: "Bash",
        result: t("states.approved"),
        detail: "cargo build --release",
      },
      {
        id: "a-2",
        time: "14:31:58",
        actor: "xujiang",
        event: "Write",
        result: t("states.rejected"),
        detail: "/etc/nginx/default.conf",
      },
      {
        id: "a-3",
        time: "14:20:03",
        actor: "alice",
        event: "WebSearch",
        result: t("states.autoAllowed"),
        detail: "rust async runtime",
      },
    ],
    [t],
  );

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("auditPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("auditPanel.detail")}</div>
        </div>
        <Button variant="secondary">{t("auditPanel.export")}</Button>
      </div>
      <div className="space-y-3">
        {auditRows.map((entry) => (
          <div className="rounded-[1.2rem] border border-border/80 bg-background/45 p-4" key={entry.id}>
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div className="flex items-center gap-3 text-sm">
                <span className="text-muted">{entry.time}</span>
                <span className="font-medium text-ink">{entry.actor}</span>
                <span className="text-ink">{entry.event}</span>
              </div>
              <Badge tone={entry.result === t("states.rejected") ? "danger" : "success"}>
                {entry.result}
              </Badge>
            </div>
            <div className="mt-2 text-sm text-muted">{entry.detail}</div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
