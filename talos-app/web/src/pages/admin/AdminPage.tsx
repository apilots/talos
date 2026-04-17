import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Activity, Bot, Cpu, ListChecks, ShieldCheck, ShieldPlus, Users } from "lucide-react";

import {
  AdminAuditSection,
  AdminDashboardSection,
  AdminModelsSection,
  AdminNavigation,
  AdminSessionsSection,
  AdminSystemSection,
  AdminToolsSection,
  AdminUsersSection,
  adminSections,
  useAdminPage,
  type AdminSection,
} from "@/features/admin";
import { Badge } from "@/shared/ui/Badge";
import { Panel } from "@/shared/ui/Panel";

export function AdminPage() {
  const { t } = useTranslation(["admin"]);
  const { sessions, models, connection, metrics, activeSection, setActiveSection, sessionSearch, setSessionSearch, sessionRows } =
    useAdminPage();
  const sectionLabels = useMemo<Record<AdminSection, string>>(
    () => ({
      dashboard: t("admin:dashboard"),
      users: t("admin:users"),
      sessions: t("admin:sessions"),
      tools: t("admin:tools"),
      audit: t("admin:audit"),
      models: t("admin:models"),
      system: t("admin:system"),
    }),
    [t],
  );
  const sectionIcons: Record<AdminSection, typeof Activity> = {
    dashboard: Activity,
    users: Users,
    sessions: ListChecks,
    tools: ShieldPlus,
    audit: ShieldCheck,
    models: Bot,
    system: Cpu,
  };
  return (
    <div className="mx-auto flex h-full w-full max-w-[1240px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.5rem] border-border/80 bg-card/88 p-6 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("admin:pageLabel")}</div>
            <h1 className="mt-2 font-display text-[30px] tracking-[-0.05em] text-ink">{t("admin:title")}</h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">{t("admin:detail")}</p>
          </div>
          <div className="flex items-center gap-3">
            <div className="text-xs text-muted">
              {t("admin:todayLabel")} · 2026-04-16
            </div>
            <Badge tone="warning">{t("admin:preview")}</Badge>
          </div>
        </div>
      </Panel>

      <div className="grid gap-5 xl:grid-cols-[220px_minmax(0,1fr)]">
        <AdminNavigation
          activeSection={activeSection}
          sectionIcons={sectionIcons}
          sectionLabels={sectionLabels}
          sections={adminSections}
          setActiveSection={setActiveSection}
        />

        <div className="space-y-5">
          {activeSection === "dashboard" ? <AdminDashboardSection connection={connection} metrics={metrics} sessions={sessions} /> : null}
          {activeSection === "users" ? <AdminUsersSection /> : null}
          {activeSection === "sessions" ? (
            <AdminSessionsSection
              connection={connection}
              sessionRows={sessionRows}
              sessionSearch={sessionSearch}
              setSessionSearch={setSessionSearch}
            />
          ) : null}
          {activeSection === "tools" ? <AdminToolsSection /> : null}
          {activeSection === "audit" ? <AdminAuditSection /> : null}
          {activeSection === "models" ? <AdminModelsSection models={models} /> : null}
          {activeSection === "system" ? <AdminSystemSection connection={connection} /> : null}
        </div>
      </div>
    </div>
  );
}
