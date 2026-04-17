import { useTranslation } from "react-i18next";

import type { AdminPageState } from "../hooks/useAdminPage";
import { formatRelativeTime } from "@/shared/lib/time";
import { Badge } from "@/shared/ui/Badge";
import { Button } from "@/shared/ui/Button";
import { Input } from "@/shared/ui/Input";
import { Panel } from "@/shared/ui/Panel";

export function AdminSessionsSection({
  connection,
  sessionRows,
  sessionSearch,
  setSessionSearch,
}: Pick<
  AdminPageState,
  "connection" | "sessionRows" | "sessionSearch" | "setSessionSearch"
>) {
  const { t } = useTranslation("admin");

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
        {sessionRows.map((session) => (
          <div
            className="grid grid-cols-[84px_minmax(0,1fr)_120px_110px_120px] items-center gap-4 rounded-[1.2rem] border border-border/80 bg-background/45 px-4 py-4"
            key={session.context.session_id}
          >
            <Badge tone="success">{t("states.running")}</Badge>
            <div className="min-w-0">
              <div className="truncate text-sm font-medium text-ink">{session.title}</div>
              <div className="mt-1 truncate text-xs text-muted">{session.context.session_id}</div>
            </div>
            <div className="text-sm text-muted">{connection.userId}</div>
            <div className="text-sm text-muted">{formatRelativeTime(session.state.last_activity)}</div>
            <div className="flex justify-end gap-2">
              <Button variant="ghost">{t("sessionsPanel.view")}</Button>
              <Button variant="danger">{t("sessionsPanel.stop")}</Button>
            </div>
          </div>
        ))}
      </div>
    </Panel>
  );
}
