import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";

import { listUsers } from "@/features/auth";
import { listAdminSessions, useConnectionKey, useModelsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";
import { useAppStore } from "@/shared/store/app-store";
import { getEffectiveSessionStatus } from "@/shared/types/domain";

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
  const connectionKey = useConnectionKey();
  const sessions = useQuery({
    queryKey: ["admin-sessions", connectionKey],
    queryFn: listAdminSessions,
  });
  const users = useQuery({
    queryKey: ["admin-users", connectionKey],
    queryFn: listUsers,
  });
  const models = useModelsQuery();
  const { connection } = useConnectionSettings();
  const metrics = useAppStore((state) => state.adminMetrics);
  const setAdminMetrics = useAppStore((state) => state.setAdminMetrics);
  const sessionAliases = useAppStore((state) => state.sessionAliases);
  const statuses = useAppStore((state) => state.transientStatuses);
  const streams = useAppStore((state) => state.streams);
  const [activeSection, setActiveSection] = useState<AdminSection>("dashboard");
  const [sessionSearch, setSessionSearch] = useState("");

  const sessionRows = useMemo(() => {
    const query = sessionSearch.trim().toLowerCase();
    const ownerLabels = new Map((users.data ?? []).map((user) => [user.user_id, user.username]));
    return (sessions.data ?? [])
      .map((session) => {
        const sessionId = session.context.session_id;
        return {
          session,
          sessionId,
          ownerUserId: session.owner_user_id ?? "",
          ownerLabel: ownerLabels.get(session.owner_user_id ?? "") ?? session.owner_user_id ?? "",
          sourceUserId: session.context.user_id,
          label: sessionAliases[sessionId] ?? session.title,
          model: session.config.llm?.model ?? session.config.agent?.model ?? "unknown",
          status: getEffectiveSessionStatus(statuses[sessionId], streams[sessionId]),
        };
      })
      .filter((row) => {
      if (!query) {
        return true;
      }
      return [
        row.label,
        row.model,
        row.sessionId,
        row.ownerLabel,
        row.ownerUserId,
        row.sourceUserId,
      ]
        .join(" ")
        .toLowerCase()
        .includes(query);
      });
  }, [sessionAliases, sessionSearch, sessions.data, statuses, streams, users.data]);

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
