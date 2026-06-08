import type { LucideIcon } from "lucide-react";
import { Bot, Brush, ServerCog, Settings2, ShieldCheck, UserCircle, UserRoundCog } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Link, Navigate, useParams } from "react-router-dom";

import {
  SettingsAccountSection,
  SettingsAgentWorkspaceSection,
  SettingsAppearanceSection,
  SettingsGeneralSection,
  SettingsModelsSection,
  SettingsPermissionsSection,
  SettingsServerSection,
  isSettingsSection,
  type SettingsSection,
  useSettingsPage,
} from "@/features/settings";
import { cn } from "@/shared/lib/cn";
import { Panel } from "@/shared/ui/Panel";

type SettingsNavigationItem = {
  id: SettingsSection;
  icon: LucideIcon;
};

const settingsNavigation: SettingsNavigationItem[] = [
  { id: "general", icon: Settings2 },
  { id: "account", icon: UserCircle },
  { id: "models", icon: Bot },
  { id: "agent-workspace", icon: UserRoundCog },
  { id: "permissions", icon: ShieldCheck },
  { id: "appearance", icon: Brush },
  { id: "advanced", icon: ServerCog },
];

export function SettingsPage() {
  const { t } = useTranslation(["settings", "common"]);
  const params = useParams();
  const {
    connection,
    setConnection,
    uiTheme,
    setUiTheme,
    locale,
    setLocale,
    draft,
    setDraft,
    desktopNotifications,
    setDesktopNotifications,
    safeFileReads,
    setSafeFileReads,
    safeFileWrites,
    setSafeFileWrites,
    models,
  } = useSettingsPage();
  const requestedSection = params.section;
  const activeSection = requestedSection ? requestedSection : "general";
  const sections = useMemo(() => settingsNavigation, []);

  if (requestedSection && !isSettingsSection(requestedSection)) {
    return <Navigate replace to="/settings" />;
  }

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto grid w-full max-w-[1280px] gap-6 px-8 py-8 xl:grid-cols-[260px_minmax(0,1fr)]">
          <Panel className="h-fit rounded-[var(--radius-panel)] border-border/75 bg-card/86 p-3">
            <div className="px-2 pb-2 pt-2">
              <div className="text-[11px] uppercase tracking-[0.22em] text-muted/80">
                {t("settings:pageLabel")}
              </div>
            </div>
            <nav className="mt-3 space-y-1">
              {sections.map((item) => {
                const Icon = item.icon;
                const isCurrent = activeSection === item.id;
                return (
                  <Link
                    aria-current={isCurrent ? "page" : undefined}
                    className={cn(
                      "flex items-center gap-3 rounded-[var(--radius-control)] px-3 py-2.5 text-sm text-muted transition hover:bg-background/55 hover:text-ink",
                      isCurrent && "bg-background/75 text-ink shadow-[inset_0_0_0_1px_rgba(224,218,209,0.5)]",
                    )}
                    key={item.id}
                    to={item.id === "general" ? "/settings" : `/settings/${item.id}`}
                  >
                    <Icon size={16} />
                    <span>{t(`settings:sections.${item.id}`)}</span>
                  </Link>
                );
              })}
            </nav>
          </Panel>

          <div className="min-w-0 space-y-5">
            {activeSection === "general" ? (
              <SettingsGeneralSection
                draft={draft}
                locale={locale}
                setDraft={setDraft}
                setLocale={setLocale}
                uiTheme={uiTheme}
              />
            ) : null}

            {activeSection === "account" ? <SettingsAccountSection connection={connection} /> : null}

            {activeSection === "models" ? <SettingsModelsSection /> : null}

            {activeSection === "agent-workspace" ? <SettingsAgentWorkspaceSection /> : null}

            {activeSection === "permissions" ? (
              <SettingsPermissionsSection
                safeFileReads={safeFileReads}
                safeFileWrites={safeFileWrites}
                setSafeFileReads={setSafeFileReads}
                setSafeFileWrites={setSafeFileWrites}
              />
            ) : null}

            {activeSection === "appearance" ? (
              <SettingsAppearanceSection
                desktopNotifications={desktopNotifications}
                setDesktopNotifications={setDesktopNotifications}
                setUiTheme={setUiTheme}
                uiTheme={uiTheme}
              />
            ) : null}

            {activeSection === "advanced" ? (
              <SettingsServerSection
                connection={connection}
                draft={draft}
                models={models}
                setConnection={setConnection}
                setDraft={setDraft}
                setLocale={setLocale}
              />
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
