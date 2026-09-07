import { Lock } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Navigate, useParams } from "react-router-dom";

import {
  SettingsPersonalizationSection,
  SettingsAppearanceSection,
  SettingsGeneralSection,
  SettingsMcpServersSection,
  SettingsModelsSection,
  SettingsNavigation,
  SettingsProfileSection,
  SettingsServerSection,
  SettingsWebSearchSection,
  isSettingsSection,
  type SettingsSection,
  useSettingsPage,
} from "@/features/settings";

export function SettingsPage() {
  const { t } = useTranslation("settings");
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
    models,
  } = useSettingsPage();
  const requestedSection = params.section;
  const activeSection: SettingsSection = isSettingsSection(requestedSection) ? requestedSection : "profile";
  const canManageConfig =
    connection.authStatus === "open" || connection.deploymentMode === "local" || connection.authRole === "admin";

  if (requestedSection && !isSettingsSection(requestedSection)) {
    return <Navigate replace to="/settings" />;
  }
  if ((activeSection === "models" || activeSection === "web-search") && !canManageConfig) {
    return <Navigate replace to="/settings" />;
  }

  return (
    <div className="grid h-full w-full grid-cols-[320px_minmax(0,1fr)] overflow-hidden bg-background">
      <SettingsNavigation
        activeSection={activeSection}
        canManageConfig={canManageConfig}
        canOpenAccount={connection.authStatus === "authenticated"}
      />
      <div className="min-h-0 overflow-y-auto">
        <div className="mx-auto w-full max-w-[1040px] px-8 py-7">
          <header className="mb-8 flex min-h-8 items-center justify-between gap-4">
            <h1 className="text-sm font-medium text-ink">{t(`sections.${activeSection}`)}</h1>
            {activeSection === "profile" ? (
              <div className="flex items-center gap-5 text-sm font-medium text-muted">
                <span className="inline-flex items-center gap-1.5">
                  <Lock aria-hidden="true" size={14} />
                  {t("profile.private")}
                </span>
              </div>
            ) : null}
          </header>
          <div className="min-w-0 space-y-5">
            {activeSection === "profile" ? <SettingsProfileSection connection={connection} /> : null}

            {activeSection === "general" ? (
              <SettingsGeneralSection
                draft={draft}
                locale={locale}
                setDraft={setDraft}
                setLocale={setLocale}
                uiTheme={uiTheme}
              />
            ) : null}

            {activeSection === "models" ? <SettingsModelsSection /> : null}

            {activeSection === "personalization" ? <SettingsPersonalizationSection /> : null}

            {activeSection === "appearance" ? (
              <SettingsAppearanceSection
                desktopNotifications={desktopNotifications}
                setDesktopNotifications={setDesktopNotifications}
                setUiTheme={setUiTheme}
                uiTheme={uiTheme}
              />
            ) : null}

            {activeSection === "mcp-servers" ? <SettingsMcpServersSection /> : null}

            {activeSection === "web-search" ? <SettingsWebSearchSection /> : null}

            {activeSection === "server" ? (
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
