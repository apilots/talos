import { Box, Folder, MessageSquare, TrendingUp } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { SettingsPageState } from "../hooks/useSettingsPage";
import { useProfileQuery } from "../hooks/useProfileQuery";
import { ProfileActivityHeatmap } from "./profile/ProfileActivityHeatmap";
import { ProfileIdentity } from "./profile/ProfileIdentity";
import { ProfileInsightsGrid, type ProfileMetricView } from "./profile/ProfileInsightsGrid";
import { ProfileStatsStrip, type ProfileStatView } from "./profile/ProfileStatsStrip";

export function SettingsProfileSection({
  connection,
}: Pick<SettingsPageState, "connection">) {
  const { t } = useTranslation(["settings", "auth"]);
  const profileQuery = useProfileQuery();
  const avatarLabel = connection.authUsername.trim().charAt(0).toUpperCase() || "A";
  const accountTypeLabel = connection.authStatus === "open" ? "Open" : connection.accountType;

  if (profileQuery.isLoading) {
    return <ProfileShell accountTypeLabel={accountTypeLabel} avatarLabel={avatarLabel} connection={connection} message={t("settings:profile.loading")} />;
  }

  if (profileQuery.error || !profileQuery.data) {
    const detail = profileQuery.error instanceof Error
      ? profileQuery.error.message
      : t("settings:profile.loadError");
    return <ProfileShell accountTypeLabel={accountTypeLabel} avatarLabel={avatarLabel} connection={connection} message={`${t("settings:profile.loadError")} ${detail}`} />;
  }

  const profile = profileQuery.data;
  const totalTokens = profile.usage.total_tokens;
  const cacheReadTokens = profile.usage.cache_read_input_tokens ?? 0;
  const mostUsedModels = [...profile.modelUsage]
    .sort((left, right) => right.usage.total_tokens - left.usage.total_tokens)
    .slice(0, 3);
  const primaryModel = mostUsedModels[0]?.modelRef.id;

  const stats: ProfileStatView[] = [
    { label: t("settings:profile.sessions"), value: formatNumber(profile.sessionCount) },
    { label: t("settings:profile.tokens"), value: formatUsage(profile.usageAvailable, totalTokens, t("settings:profile.unavailable")) },
    { label: t("settings:profile.messages"), value: formatNumber(profile.messageCount) },
    { label: t("settings:profile.projects"), value: formatNumber(profile.projectCount) },
    { label: t("settings:profile.cacheRead"), value: formatUsage(profile.usageAvailable, cacheReadTokens, t("settings:profile.unavailable")) },
  ];
  const usageRows: ProfileMetricView[] = [
    {
      icon: <Box size={16} />,
      label: t("settings:profile.mostUsedModel"),
      value: primaryModel ?? t("settings:profile.noModelUsage"),
    },
    { icon: <TrendingUp size={16} />, label: t("settings:profile.averageTokensSession"), value: profile.usageAvailable ? formatNumber(averageTokens(totalTokens, profile.usageSessionCount)) : t("settings:profile.unavailable") },
    { icon: <Folder size={16} />, label: t("settings:profile.activeProjects"), value: formatNumber(profile.projectCount) },
  ];
  const modelRows: ProfileMetricView[] =
    mostUsedModels.length > 0
      ? mostUsedModels.map((model) => ({
          icon: <Box size={16} />,
          label: model.modelRef.id,
          value: t("settings:profile.runs", { count: model.turnCount }),
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
      <ProfileIdentity accountTypeLabel={accountTypeLabel} avatarLabel={avatarLabel} displayName={displayName(connection.authUsername)} openMode={connection.authStatus === "open"} username={connection.authUsername} />
      <ProfileStatsStrip stats={stats} />
      <ProfileActivityHeatmap
        activity={profile.dailyUsage
          .filter((day) => day.usageAvailable)
          .map((day) => ({ date: day.date, tokens: day.usage.total_tokens }))}
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

function ProfileShell({
  accountTypeLabel,
  avatarLabel,
  connection,
  message,
}: Pick<SettingsProfileSectionProps, "connection"> & {
  accountTypeLabel: string;
  avatarLabel: string;
  message: string;
}) {
  return (
    <div className="mx-auto max-w-[960px]">
      <ProfileIdentity accountTypeLabel={accountTypeLabel} avatarLabel={avatarLabel} displayName={displayName(connection.authUsername)} openMode={connection.authStatus === "open"} username={connection.authUsername} />
      <p className="mx-auto mt-10 max-w-[760px] rounded-[var(--radius-control)] border border-danger/30 bg-danger/5 px-4 py-3 text-sm text-danger" role="alert">{message}</p>
    </div>
  );
}

type SettingsProfileSectionProps = Pick<SettingsPageState, "connection">;

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

function formatUsage(available: boolean, value: number, unavailable: string) {
  return available ? formatCompact(value) : unavailable;
}
