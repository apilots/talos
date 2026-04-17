import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { useModelsQuery, useSessionsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";
import { useAppStore } from "@/shared/store/app-store";

export type AdminSection = "dashboard" | "users" | "sessions" | "tools" | "audit" | "models" | "system";

export const adminSections: AdminSection[] = [
  "dashboard",
  "users",
  "sessions",
  "tools",
  "audit",
  "models",
  "system",
];

export function useAdminPage() {
  const { t } = useTranslation("admin");
  const sessions = useSessionsQuery();
  const models = useModelsQuery();
  const { connection } = useConnectionSettings();
  const metrics = useAppStore((state) => state.adminMetrics);
  const setAdminMetrics = useAppStore((state) => state.setAdminMetrics);
  const [activeSection, setActiveSection] = useState<AdminSection>("dashboard");
  const [sessionSearch, setSessionSearch] = useState("");

  const sessionRows = useMemo(() => {
    const query = sessionSearch.trim().toLowerCase();
    return (sessions.data ?? []).filter((session) => {
      if (!query) {
        return true;
      }
      return [session.title, session.config.llm?.model ?? "", session.context.session_id]
        .join(" ")
        .toLowerCase()
        .includes(query);
    });
  }, [sessionSearch, sessions.data]);

  useEffect(() => {
    setAdminMetrics([
      {
        id: "active-sessions",
        label: t("metrics.activeSessionsLabel"),
        value: String(sessions.data?.length ?? 0),
        trend: t("metrics.activeSessionsTrend"),
      },
      {
        id: "model-endpoints",
        label: t("metrics.configuredModelsLabel"),
        value: String(models.data?.length ?? 0),
        trend: t("metrics.configuredModelsTrend"),
      },
      {
        id: "pending-approvals",
        label: t("metrics.pendingApprovalsLabel"),
        value: "2",
        trend: t("metrics.pendingApprovalsTrend"),
      },
      {
        id: "operational-risk",
        label: t("metrics.operationalRiskLabel"),
        value: t("states.low"),
        trend: t("metrics.operationalRiskTrend"),
      },
    ]);
  }, [models.data?.length, sessions.data?.length, setAdminMetrics, t]);

  return {
    sessions,
    models,
    connection,
    metrics,
    activeSection,
    setActiveSection,
    sessionSearch,
    setSessionSearch,
    sessionRows,
  };
}

export type AdminPageState = ReturnType<typeof useAdminPage>;
