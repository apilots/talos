import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";

import type { AdminPageState } from "../hooks/useAdminPage";
import { formatRelativeTime } from "@/shared/lib/time";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

export function AdminSessionsSection({
  sessionRows,
  sessionSearch,
  setSessionSearch,
}: Pick<
  AdminPageState,
  "sessionRows" | "sessionSearch" | "setSessionSearch"
>) {
  const { t } = useTranslation("admin");
  const navigate = useNavigate();

  function sessionTone(status: "running" | "waiting_approval" | "done" | "error") {
    if (status === "running") {
      return "success";
    }
    if (status === "waiting_approval") {
      return "warning";
    }
    if (status === "error") {
      return "danger";
    }
    return "default";
  }

  function sessionLabel(status: "running" | "waiting_approval" | "done" | "error") {
    if (status === "waiting_approval") {
      return t("states.needsApproval");
    }
    if (status === "error") {
      return t("states.rejected");
    }
    if (status === "done") {
      return t("states.done");
    }
    return t("states.running");
  }

  return (
    <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-sm font-medium text-ink">{t("sessionsPanel.title")}</div>
          <div className="mt-1 text-sm text-muted">{t("sessionsPanel.detail")}</div>
        </div>
        <Input
          className="max-w-xs"
          onChange={(event) => setSessionSearch(event.target.value)}
          placeholder={t("sessionsPanel.searchPlaceholder")}
          value={sessionSearch}
        />
      </div>
      <div className="space-y-3">
        <div className="grid grid-cols-[110px_minmax(0,1fr)_150px_96px_132px] gap-4 px-4 text-xs uppercase tracking-[0.16em] text-muted">
          <div>{t("sessionsPanel.status")}</div>
          <div>{t("sessionsPanel.session")}</div>
          <div>{t("sessionsPanel.owner")}</div>
          <div className="text-right">{t("sessionsPanel.actions")}</div>
          <div>{t("sessionsPanel.updated")}</div>
        </div>
        {sessionRows.map((row) => (
          <div
            className="grid grid-cols-[110px_minmax(0,1fr)_150px_96px_132px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
            key={row.session.context.session_id}
          >
            <Badge tone={sessionTone(row.status)}>{sessionLabel(row.status)}</Badge>
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-ink">{row.label}</div>
              <div className="mt-1 truncate text-xs text-muted">{row.session.context.session_id}</div>
            </div>
            <div className="min-w-0 text-sm text-muted">{row.ownerLabel}</div>
            <div className="flex justify-end">
              <Button
                onClick={() => navigate(`/sessions/${row.session.context.session_id}`)}
                type="button"
                variant="ghost"
              >
                {t("sessionsPanel.view")}
              </Button>
            </div>
            <div className="text-sm text-muted">{formatRelativeTime(row.session.state.last_activity)}</div>
          </div>
        ))}
        {sessionRows.length === 0 ? (
          <div className="rounded-[1.2rem] border border-dashed border-border/80 px-4 py-8 text-sm text-muted">
            {t("sessionsPanel.empty")}
          </div>
        ) : null}
      </div>
    </Panel>
  );
}
