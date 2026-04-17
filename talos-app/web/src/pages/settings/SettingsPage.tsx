import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import {
  SettingsAppearanceSection,
  SettingsGeneralSection,
  SettingsKeysSection,
  SettingsModelsSection,
  SettingsServerSection,
  useSettingsPage,
} from "@/features/settings";
import { Badge } from "@/shared/ui/Badge";
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
    defaultModel,
    setDefaultModel,
    desktopNotifications,
    setDesktopNotifications,
    safeFileReads,
    setSafeFileReads,
    safeFileWrites,
    setSafeFileWrites,
    terminalApproval,
    setTerminalApproval,
    models,
    modelCards,
  } = useSettingsPage();
  const tabs = useMemo(
    () => [
      { id: "general", label: t("settings:tabs.general") },
      { id: "models", label: t("settings:tabs.models") },
      { id: "keys", label: t("settings:tabs.keys") },
      { id: "appearance", label: t("settings:tabs.appearance") },
      { id: "server", label: t("settings:tabs.server") },
    ],
    [t],
  );

  return (
    <div className="mx-auto flex h-full w-full max-w-[1120px] flex-col gap-5 overflow-y-auto px-8 py-7">
      <Panel className="rounded-[1.45rem] border-border/80 bg-card/88 p-5 backdrop-blur">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="text-xs uppercase tracking-[0.22em] text-muted">{t("settings:pageLabel")}</div>
            <h1 className="mt-2 font-display text-[28px] tracking-[-0.05em] text-ink">{t("settings:title")}</h1>
            <p className="mt-3 max-w-2xl text-sm leading-6 text-muted">{t("settings:detail")}</p>
          </div>
          <Badge tone={models.isSuccess ? "success" : "warning"}>
            {models.isSuccess ? t("common:connectedShort") : t("common:awaitingServer")}
          </Badge>
        </div>
      </Panel>

      <Panel className="rounded-[1.3rem] border-border/80 bg-card/82 p-2">
        <div className="flex flex-wrap gap-2">
          {tabs.map((tab) => (
            <button
              className={[
                "rounded-2xl px-4 py-2.5 text-sm transition",
                activeTab === tab.id
                  ? "bg-background/70 text-ink shadow-panel"
                  : "text-muted hover:bg-background/45 hover:text-ink",
              ].join(" ")}
              key={tab.id}
              onClick={() => setActiveTab(tab.id as "general" | "models" | "keys" | "appearance" | "server")}
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

      {activeTab === "models" ? (
        <SettingsModelsSection defaultModel={defaultModel} modelCards={modelCards} models={models} setDefaultModel={setDefaultModel} />
      ) : null}

      {activeTab === "keys" ? <SettingsKeysSection /> : null}

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
  );
}
