import { Activity, AlertTriangle, Cpu, Gauge } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import type { AdminPageState } from "../hooks/useAdminPage";
import { formatRelativeTime } from "@/shared/lib/time";
import { Badge } from "@/shared/ui/Badge";
import { Panel } from "@/shared/ui/Panel";

export function AdminDashboardSection({
  connection,
  metrics,
  sessions,
}: Pick<AdminPageState, "connection" | "metrics" | "sessions">) {
  const { t } = useTranslation("admin");
  const topTools = useMemo(
    () =>
      [
        ["Read", 1203],
        ["Bash", 421],
        ["WebSearch", 389],
        ["Write", 201],
        ["WebFetch", 98],
      ] as const,
    [],
  );

  return (
    <>
      <div className="grid gap-4 xl:grid-cols-4">
        {metrics.map((metric, index) => {
          const Icon = [Activity, Gauge, AlertTriangle, Cpu][index % 4];
          return (
            <Panel className="rounded-[1.35rem] border-border/80 bg-card/82 p-5" key={metric.id}>
              <div className="flex items-center justify-between gap-3">
                <div className="text-xs uppercase tracking-[0.18em] text-muted">{metric.label}</div>
                <Icon className="text-brand" size={16} />
              </div>
              <div className="mt-4 font-display text-4xl tracking-[-0.05em] text-ink">
                {metric.value}
              </div>
              <p className="mt-3 text-sm text-muted">{metric.trend}</p>
            </Panel>
          );
        })}
      </div>
      <div className="grid gap-5 xl:grid-cols-[1.2fr_0.9fr]">
        <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
          <div className="text-sm font-medium text-ink">{t("dashboardCards.activeAgentTasks")}</div>
          <div className="mt-4 space-y-3">
            {(sessions.data ?? []).slice(0, 3).map((session) => (
              <div
                className="rounded-[1.15rem] border border-border/80 bg-background/45 p-4"
                key={session.context.session_id}
              >
                <div className="flex items-center justify-between gap-3">
                  <div className="text-sm font-medium text-ink">{session.title}</div>
                  <Badge tone="success">{t("states.running")}</Badge>
                </div>
                <div className="mt-2 text-xs text-muted">
                  User {connection.userId} · {formatRelativeTime(session.state.last_activity)}
                </div>
              </div>
            ))}
          </div>
        </Panel>
        <Panel className="rounded-[1.4rem] border-border/80 bg-card/82 p-5">
          <div className="text-sm font-medium text-ink">{t("dashboardCards.topTools")}</div>
          <div className="mt-4 space-y-4">
            {topTools.map(([label, value]) => (
              <div key={label}>
                <div className="mb-2 flex items-center justify-between text-sm">
                  <span className="text-ink">{label}</span>
                  <span className="text-muted">{value}</span>
                </div>
                <div className="h-2 rounded-full bg-background/70">
                  <div
                    className="h-2 rounded-full bg-brand/75"
                    style={{ width: `${(value / 1203) * 100}%` }}
                  />
                </div>
              </div>
            ))}
          </div>
        </Panel>
      </div>
    </>
  );
}
