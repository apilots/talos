import type { LucideIcon } from "lucide-react";
import {
  ArrowLeft,
  Bot,
  Boxes,
  Brush,
  ServerCog,
  Settings2,
  Search,
  UserCircle,
  UserRoundCog,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { useState } from "react";
import { Link } from "react-router-dom";

import {
  closePreparedExternalWindow,
  getCloudAuthUrl,
  prepareExternalAuthWindow,
  redirectToExternalUrl,
} from "@/features/auth";
import { cn } from "@/shared/lib/cn";
import type { SettingsSection } from "../hooks/useSettingsPage";

type InternalSettingsNavigationItem = {
  id: SettingsSection;
  icon: LucideIcon;
  external?: false;
};

type ExternalSettingsNavigationItem = {
  id: "account";
  icon: LucideIcon;
  external: true;
};

export type SettingsNavigationItem = InternalSettingsNavigationItem | ExternalSettingsNavigationItem;

type SettingsNavigationProps = {
  activeSection: SettingsSection;
  canOpenAccount: boolean;
  canManageConfig: boolean;
};

const personalNavigation: SettingsNavigationItem[] = [
  { id: "profile", icon: UserCircle },
  { id: "general", icon: Settings2 },
  { id: "account", icon: UserCircle, external: true },
  { id: "appearance", icon: Brush },
  { id: "models", icon: Bot },
  { id: "agent-workspace", icon: UserRoundCog },
];

const extensionNavigation: SettingsNavigationItem[] = [
  { id: "web-search", icon: Search },
  { id: "mcp-servers", icon: Boxes },
];

const advancedNavigation: SettingsNavigationItem[] = [
  { id: "server", icon: ServerCog },
];

export const settingsNavigation: SettingsNavigationItem[] = [
  ...personalNavigation,
  ...extensionNavigation,
  ...advancedNavigation,
];

export function SettingsNavigation({ activeSection, canManageConfig, canOpenAccount }: SettingsNavigationProps) {
  const { t } = useTranslation(["settings", "common"]);
  const [accountOpenError, setAccountOpenError] = useState(false);
  const visiblePersonalNavigation = canManageConfig
    ? personalNavigation
    : personalNavigation.filter((item) => item.id !== "models");
  const visibleExtensionNavigation = canManageConfig
    ? extensionNavigation
    : extensionNavigation.filter((item) => item.id !== "web-search");
  const openAccount = () => {
    const preparedWindow = prepareExternalAuthWindow();
    setAccountOpenError(false);
    void getCloudAuthUrl()
      .then((cloudAuthUrl) => {
        const targetUrl = new URL("/account", cloudAuthUrl).toString();
        redirectToExternalUrl(targetUrl, preparedWindow);
      })
      .catch((error: unknown) => {
        closePreparedExternalWindow(preparedWindow);
        setAccountOpenError(true);
        console.error("Failed to open Cloud account management.", error);
      });
  };

  return (
    <aside className="flex h-screen w-full flex-col border-r border-border/70 bg-background px-4 py-5">
      <Link
        className="mb-7 inline-flex items-center gap-2 rounded-[var(--radius-control)] px-2 py-2 text-sm text-muted transition hover:bg-card/80 hover:text-ink"
        to="/"
      >
        <ArrowLeft size={15} />
        <span>{t("settings:backToApp")}</span>
      </Link>
      <nav className="space-y-1">
        <NavigationGroup
          activeSection={activeSection}
          canOpenAccount={canOpenAccount}
          items={visiblePersonalNavigation}
          label={t("settings:navigation.personal")}
          onAccountClick={openAccount}
        />
        <NavigationGroup
          activeSection={activeSection}
          canOpenAccount={canOpenAccount}
          items={visibleExtensionNavigation}
          label={t("settings:navigation.extensions")}
          onAccountClick={openAccount}
        />
        <NavigationGroup
          activeSection={activeSection}
          canOpenAccount={canOpenAccount}
          items={advancedNavigation}
          label={t("settings:navigation.advanced")}
          onAccountClick={openAccount}
        />
      </nav>
      {accountOpenError ? (
        <p className="px-3 text-xs leading-5 text-danger" role="alert">
          {t("settings:account.openError")}
        </p>
      ) : null}
    </aside>
  );
}

function NavigationGroup({
  activeSection,
  canOpenAccount,
  items,
  label,
  onAccountClick,
}: {
  activeSection: SettingsSection;
  canOpenAccount: boolean;
  items: SettingsNavigationItem[];
  label: string;
  onAccountClick: () => void;
}) {
  const { t } = useTranslation("settings");

  return (
    <section className="mb-4">
      <div className="mb-2 px-2 text-xs font-medium text-muted">{label}</div>
      <div className="space-y-1">
        {items.map((item) => {
          const Icon = item.icon;
          const isCurrent = item.external ? false : activeSection === item.id;
          const className = cn(
            "flex items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-sm text-muted transition hover:bg-card/88 hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/30",
            isCurrent && "bg-card/95 text-ink shadow-[inset_0_0_0_1px_rgba(224,218,209,0.55)]",
            item.external && "disabled:cursor-not-allowed disabled:opacity-55",
          );

          if (item.external) {
            return (
              <button
                className={className}
                disabled={!canOpenAccount}
                key={item.id}
                onClick={onAccountClick}
                type="button"
              >
                <Icon aria-hidden="true" size={16} />
                <span>{t(`sections.${item.id}`)}</span>
              </button>
            );
          }

          return (
            <Link
              aria-current={isCurrent ? "page" : undefined}
              className={className}
              key={item.id}
              to={item.id === "profile" ? "/settings" : `/settings/${item.id}`}
            >
              <Icon aria-hidden="true" size={16} />
              <span>{t(`sections.${item.id}`)}</span>
            </Link>
          );
        })}
      </div>
    </section>
  );
}
