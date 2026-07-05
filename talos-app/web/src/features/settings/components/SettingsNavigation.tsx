import type { LucideIcon } from "lucide-react";
import {
  ArrowLeft,
  Bot,
  Boxes,
  Brush,
  ServerCog,
  Settings2,
  ShieldCheck,
  UserCircle,
  UserRoundCog,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { cn } from "@/shared/lib/cn";
import type { SettingsSection } from "../hooks/useSettingsPage";

export type SettingsNavigationItem = {
  id: SettingsSection;
  icon: LucideIcon;
};

const personalNavigation: SettingsNavigationItem[] = [
  { id: "profile", icon: UserCircle },
  { id: "general", icon: Settings2 },
  { id: "account", icon: UserCircle },
  { id: "appearance", icon: Brush },
  { id: "models", icon: Bot },
  { id: "agent-workspace", icon: UserRoundCog },
  { id: "permissions", icon: ShieldCheck },
];

const extensionNavigation: SettingsNavigationItem[] = [
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

interface SettingsNavigationProps {
  activeSection: SettingsSection;
}

export function SettingsNavigation({ activeSection }: SettingsNavigationProps) {
  const { t } = useTranslation(["settings", "common"]);

  return (
    <aside className="flex h-screen w-full flex-col border-r border-border/70 bg-card/95 px-4 py-5">
      <Link
        className="mb-7 inline-flex items-center gap-2 rounded-[var(--radius-control)] px-2 py-2 text-sm text-muted transition hover:bg-card/80 hover:text-ink"
        to="/"
      >
        <ArrowLeft size={15} />
        <span>{t("settings:backToApp")}</span>
      </Link>
      <nav className="space-y-1">
        <NavigationGroup activeSection={activeSection} items={personalNavigation} label={t("settings:navigation.personal")} />
        <NavigationGroup activeSection={activeSection} items={extensionNavigation} label={t("settings:navigation.extensions")} />
        <NavigationGroup activeSection={activeSection} items={advancedNavigation} label={t("settings:navigation.advanced")} />
      </nav>
    </aside>
  );
}

function NavigationGroup({
  activeSection,
  items,
  label,
}: {
  activeSection: SettingsSection;
  items: SettingsNavigationItem[];
  label: string;
}) {
  const { t } = useTranslation("settings");

  return (
    <section className="mb-4">
      <div className="mb-2 px-2 text-xs font-medium text-muted">{label}</div>
      <div className="space-y-1">
        {items.map((item) => {
          const Icon = item.icon;
          const isCurrent = activeSection === item.id;
          return (
            <Link
              aria-current={isCurrent ? "page" : undefined}
              className={cn(
                "flex items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-sm text-muted transition hover:bg-card/88 hover:text-ink focus-visible:ring-2 focus-visible:ring-brand/30",
                isCurrent && "bg-card/95 text-ink shadow-[inset_0_0_0_1px_rgba(224,218,209,0.55)]",
              )}
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
