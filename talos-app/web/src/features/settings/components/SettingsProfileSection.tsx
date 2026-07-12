import { Box, Folder, MessageSquare, TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";

import { useProjectsView, useSessionsQuery } from "@/features/chat";
import type { SettingsPageState } from "../hooks/useSettingsPage";
import { ProfileActivityHeatmap } from "./profile/ProfileActivityHeatmap";
import { ProfileIdentity } from "./profile/ProfileIdentity";
import { ProfileInsightsGrid, type ProfileMetricView } from "./profile/ProfileInsightsGrid";
import { ProfileStatsStrip, type ProfileStatView } from "./profile/ProfileStatsStrip";

export function SettingsProfileSection({
  connection,
}: Pick<SettingsPageState, "connection">) {
  const { t } = useTranslation(["settings", "auth"]);
  const sessionsQuery = useSessionsQuery();
  const projectsView = useProjectsView();
  const projects = projectsView.projects;
  const sessions = sessionsQuery.data ?? [];
  const totalTokens = sessions.reduce((sum, session) => sum + session.usage.total_tokens, 0);
  const messageCount = sessions.reduce((sum, session) => sum + session.message_count, 0);
  const cacheReadTokens = sessions.reduce((sum, session) => sum + (session.usage.cache_read_input_tokens ?? 0), 0);
  const modelsById = new Map<string, number>();
  sessions.forEach((session) => {
    const modelId = session.modelSelection.modelRef.id;
    modelsById.set(modelId, (modelsById.get(modelId) ?? 0) + 1);
  });
  const mostUsedModels = [...modelsById.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, 3);
  const primaryModel = mostUsedModels[0]?.[0];
  const avatarLabel = connection.authUsername.trim().charAt(0).toUpperCase() || "A";
  const accountTypeLabel = connection.authStatus === "open" ? "Open" : connection.accountType;

  const stats: ProfileStatView[] = [
    { label: t("settings:profile.sessions"), value: formatNumber(sessions.length) },
    { label: t("settings:profile.tokens"), value: formatCompact(totalTokens) },
    { label: t("settings:profile.messages"), value: formatNumber(messageCount) },
    { label: t("settings:profile.projects"), value: formatNumber(projects.length) },
    { label: t("settings:profile.cacheRead"), value: formatCompact(cacheReadTokens) },
  ];
  const usageRows: ProfileMetricView[] = [
    {
      icon: <Box size={16} />,
      label: t("settings:profile.mostUsedModel"),
      value: primaryModel ?? t("settings:profile.noModelUsage"),
    },
    { icon: <TrendingUp size={16} />, label: t("settings:profile.averageTokensSession"), value: formatNumber(averageTokens(totalTokens, sessions.length)) },
    { icon: <Folder size={16} />, label: t("settings:profile.activeProjects"), value: formatNumber(projects.length) },
  ];
  const modelRows: ProfileMetricView[] =
    mostUsedModels.length > 0
      ? mostUsedModels.map(([modelId, count]) => ({
          icon: <Box size={16} />,
          label: modelId,
          value: t("settings:profile.runs", { count }),
        }))
      : [
          {
            icon: <MessageSquare size={16} />,
            label: t("settings:profile.noModelUsage"),
            value: "",
          },
        ];

  return (
    <div className="mx-auto max-w-[960px]">
      <ProfileIdentity
        accountTypeLabel={accountTypeLabel}
        avatarLabel={avatarLabel}
        displayName={displayName(connection.authUsername)}
        openMode={connection.authStatus === "open"}
        username={connection.authUsername}
      />
      <ProfileStatsStrip stats={stats} />
      <ProfileActivityHeatmap
        activity={sessions.map((session) => ({
          timestamp: session.last_activity,
          tokens: session.usage.total_tokens,
        }))}
        cumulativeLabel={t("settings:profile.cumulative")}
        dailyLabel={t("settings:profile.daily")}
        title={t("settings:profile.tokenActivity")}
        weeklyLabel={t("settings:profile.weekly")}
      />
      <ProfileInsightsGrid
        leftRows={usageRows}
        leftTitle={t("settings:profile.usageInsights")}
        rightRows={modelRows}
        rightTitle={t("settings:profile.mostUsedModels")}
      />
    </div>
  );
}

function averageTokens(totalTokens: number, sessionCount: number) {
  if (sessionCount === 0) {
    return 0;
  }
  return Math.round(totalTokens / sessionCount);
}

function displayName(username: string) {
  if (username.includes("@")) {
    return username.split("@")[0] ?? username;
  }
  return username;
}

function formatNumber(value: number) {
  return new Intl.NumberFormat("en-US").format(value);
}

function formatCompact(value: number) {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(value);
}
