import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Activity, Cpu, ListChecks, ShieldCheck, ShieldPlus } from "lucide-react";

import {
  AdminAuditSection,
  AdminDashboardSection,
  AdminNavigation,
  AdminSessionsSection,
  AdminSystemSection,
  AdminToolsSection,
  adminSections,
  useAdminPage,
  type AdminSection,
} from "@/features/admin";

export function AdminPage() {
  const { t } = useTranslation(["admin"]);
  const {
    sessions,
    connection,
    metrics,
    activeSection,
    setActiveSection,
    sessionSearch,
    setSessionSearch,
    sessionPagination,
    setSessionPage,
    sessionRows,
  } = useAdminPage();
  const sectionLabels = useMemo<Record<AdminSection, string>>(
    () => ({
      dashboard: t("admin:dashboard"),
      sessions: t("admin:sessions"),
      tools: t("admin:tools"),
      audit: t("admin:audit"),
      system: t("admin:system"),
    }),
    [t],
  );
  const sectionIcons: Record<AdminSection, typeof Activity> = {
    dashboard: Activity,
    sessions: ListChecks,
    tools: ShieldPlus,
    audit: ShieldCheck,
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
              {activeSection === "sessions" ? (
                <AdminSessionsSection
                  sessionRows={sessionRows}
                  sessionSearch={sessionSearch}
                  setSessionSearch={setSessionSearch}
                  sessionPagination={sessionPagination}
                  setSessionPage={setSessionPage}
                  sessions={sessions}
                />
              ) : null}
              {activeSection === "tools" ? <AdminToolsSection /> : null}
              {activeSection === "audit" ? <AdminAuditSection /> : null}
              {activeSection === "system" ? <AdminSystemSection connection={connection} /> : null}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
