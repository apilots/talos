import { useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import {
  SettingsAppearanceSection,
  SettingsGeneralSection,
  SettingsKeysSection,
  SettingsServerSection,
  useSettingsPage,
} from "@/features/settings";
import { Panel } from "@/shared/ui/Panel";

export function SettingsPage() {
  const { t } = useTranslation(["settings", "common"]);
  const {
    connection,
    setConnection,
    uiTheme,
    setUiTheme,
    locale,
    setLocale,
    activeTab,
    setActiveTab,
    draft,
    setDraft,
    desktopNotifications,
    setDesktopNotifications,
    safeFileReads,
    setSafeFileReads,
    safeFileWrites,
    setSafeFileWrites,
    terminalApproval,
    setTerminalApproval,
    models,
  } = useSettingsPage();
  const tabs = useMemo(
    () =>
      [
        { id: "general", label: t("settings:tabs.general") },
        connection.authStatus === "open" ? null : { id: "keys", label: t("settings:tabs.keys") },
        { id: "appearance", label: t("settings:tabs.appearance") },
        { id: "server", label: t("settings:tabs.server") },
      ].filter(Boolean) as Array<{ id: string; label: string }>,
    [connection.authStatus, t],
  );

  useEffect(() => {
    if (connection.authStatus === "open" && activeTab === "keys") {
      setActiveTab("general");
    }
  }, [activeTab, connection.authStatus, setActiveTab]);

  return (
    <div className="flex h-full w-full flex-col overflow-hidden">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex w-full max-w-[1180px] flex-col gap-6 px-8 py-8">
      <Panel className="rounded-[var(--radius-panel)] border-border/75 bg-card/86 p-2">
        <div className="flex flex-wrap gap-2">
          {tabs.map((tab) => (
            <button
              className={[
                "rounded-[var(--radius-control)] px-4 py-2.5 text-sm transition",
                activeTab === tab.id
                  ? "bg-background/70 text-ink shadow-panel"
                  : "text-muted hover:bg-background/45 hover:text-ink",
              ].join(" ")}
              key={tab.id}
              onClick={() => setActiveTab(tab.id as "general" | "keys" | "appearance" | "server")}
              type="button"
            >
              {tab.label}
            </button>
          ))}
        </div>
      </Panel>

      {activeTab === "general" ? (
        <SettingsGeneralSection draft={draft} locale={locale} setDraft={setDraft} setLocale={setLocale} uiTheme={uiTheme} />
      ) : null}

      {activeTab === "keys" && connection.authStatus !== "open" ? <SettingsKeysSection /> : null}

      {activeTab === "appearance" ? (
        <SettingsAppearanceSection
          desktopNotifications={desktopNotifications}
          setDesktopNotifications={setDesktopNotifications}
          setUiTheme={setUiTheme}
          uiTheme={uiTheme}
        />
      ) : null}

      {activeTab === "server" ? (
        <SettingsServerSection
          connection={connection}
          draft={draft}
          models={models}
          safeFileReads={safeFileReads}
          safeFileWrites={safeFileWrites}
          setConnection={setConnection}
          setDraft={setDraft}
          setLocale={setLocale}
          setSafeFileReads={setSafeFileReads}
          setSafeFileWrites={setSafeFileWrites}
          setTerminalApproval={setTerminalApproval}
          terminalApproval={terminalApproval}
        />
      ) : null}
        </div>
      </div>
    </div>
  );
}
