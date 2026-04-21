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

export function AdminPage() {
  const { t } = useTranslation(["admin"]);
  const { sessions, connection, metrics, activeSection, setActiveSection, sessionSearch, setSessionSearch, sessionRows } =
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
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1240px] flex-col gap-5 px-8 py-7">
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
              sessionRows={sessionRows}
              sessionSearch={sessionSearch}
              setSessionSearch={setSessionSearch}
            />
          ) : null}
          {activeSection === "tools" ? <AdminToolsSection /> : null}
          {activeSection === "audit" ? <AdminAuditSection /> : null}
          {activeSection === "models" ? <AdminModelsSection /> : null}
          {activeSection === "system" ? <AdminSystemSection connection={connection} /> : null}
        </div>
      </div>
        </div>
      </div>
    </div>
  );
}
