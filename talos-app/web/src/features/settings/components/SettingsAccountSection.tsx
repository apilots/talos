import { ExternalLink, KeyRound, ShieldCheck, UserCircle } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { getCloudAuthUrl } from "@/features/auth";
import type { SettingsPageState } from "../hooks/useSettingsPage";
import { Panel } from "@/shared/ui/Panel";
import { OverviewCard, SectionHeader } from "./SectionPrimitives";

type CloudAccountPath = "/account" | "/tokens" | "/admin/users";

type AccountAction = {
  key: string;
  path: CloudAccountPath;
  title: string;
  detail: string;
  icon: typeof UserCircle;
  adminOnly?: boolean;
};

export function SettingsAccountSection({
  connection,
}: Pick<SettingsPageState, "connection">) {
  const { t } = useTranslation(["settings", "auth"]);
  const [cloudAuthUrl, setCloudAuthUrl] = useState<string | null>(null);
  const [cloudAuthError, setCloudAuthError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    getCloudAuthUrl()
      .then((value) => {
        if (!cancelled) {
          setCloudAuthUrl(value);
          setCloudAuthError(null);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setCloudAuthUrl(null);
          setCloudAuthError(error instanceof Error ? error.message : String(error));
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const isAuthenticated = connection.authStatus === "authenticated";
  const isAdmin = connection.authRole === "admin";
  const roleLabel = isAdmin ? t("auth:roleAdmin") : t("auth:roleUser");
  const identityLabel = connection.authUsername || connection.userId || t("settings:account.unknownUser");
  const canOpenCloud = Boolean(cloudAuthUrl && isAuthenticated);
  const cloudHost = cloudAuthUrl ? formatCloudHost(cloudAuthUrl) : t("settings:account.notConfigured");

  const actions = useMemo<AccountAction[]>(
    () => [
      {
        key: "profile",
        path: "/account",
        title: t("settings:account.manageProfile"),
        detail: t("settings:account.manageProfileDetail"),
        icon: UserCircle,
      },
      {
        key: "tokens",
        path: "/tokens",
        title: t("settings:account.manageTokens"),
        detail: t("settings:account.manageTokensDetail"),
        icon: KeyRound,
      },
      {
        key: "users",
        path: "/admin/users",
        title: t("settings:account.manageUsers"),
        detail: t("settings:account.manageUsersDetail"),
        icon: ShieldCheck,
        adminOnly: true,
      },
    ],
    [t],
  );

  function openCloud(path: CloudAccountPath) {
    if (!cloudAuthUrl || !isAuthenticated) {
      return;
    }

    const targetUrl = new URL(path, cloudAuthUrl);
    window.open(targetUrl.toString(), "_blank", "noopener,noreferrer");
  }

  return (
    <div className="space-y-5">
      <div className="grid gap-4 md:grid-cols-3">
        <OverviewCard
          detail={t("settings:account.identityDetail")}
          title={t("settings:account.identity")}
          value={identityLabel}
        />
        <OverviewCard
          detail={t("settings:account.roleDetail")}
          title={t("settings:account.role")}
          value={roleLabel}
        />
        <OverviewCard
          detail={canOpenCloud ? t("settings:account.cloudReady") : t("settings:account.cloudUnavailable")}
          title={t("settings:account.cloud")}
          value={cloudHost}
        />
      </div>

      <Panel className="rounded-[1.45rem] border-border/80 bg-card/82 p-6">
        <SectionHeader
          icon={<UserCircle size={18} />}
          subtitle={t("settings:account.detail")}
          title={t("settings:account.title")}
        />

        <div className="mt-6 grid gap-3 lg:grid-cols-3">
          {actions
            .filter((action) => !action.adminOnly || isAdmin)
            .map((action) => {
              const Icon = action.icon;
              return (
                <button
                  className="group flex min-h-[150px] flex-col justify-between rounded-[var(--radius-panel)] border border-border/80 bg-background/48 p-4 text-left transition hover:border-brand/35 hover:bg-background/70 disabled:cursor-not-allowed disabled:opacity-55"
                  disabled={!canOpenCloud}
                  key={action.key}
                  onClick={() => openCloud(action.path)}
                  type="button"
                >
                  <span className="flex items-center justify-between gap-3">
                    <span className="flex size-10 items-center justify-center rounded-[var(--radius-control)] border border-border bg-card text-brand">
                      <Icon size={18} />
                    </span>
                    <ExternalLink className="text-muted transition group-hover:text-brand" size={16} />
                  </span>
                  <span>
                    <span className="block text-sm font-medium text-ink">{action.title}</span>
                    <span className="mt-2 block text-xs leading-5 text-muted">{action.detail}</span>
                  </span>
                </button>
              );
            })}
        </div>

        {!isAuthenticated ? (
          <p className="mt-4 text-xs leading-5 text-muted">{t("settings:account.loginRequired")}</p>
        ) : null}

        {cloudAuthError ? (
          <p className="mt-4 text-xs leading-5 text-danger">
            {t("settings:account.cloudError", { message: cloudAuthError })}
          </p>
        ) : null}
      </Panel>
    </div>
  );
}

function formatCloudHost(value: string): string {
  try {
    return new URL(value).host;
  } catch {
    return value;
  }
}
