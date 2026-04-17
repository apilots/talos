import { useEffect, useMemo, useState } from "react";

import { useModelsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";

export type SettingsTab = "general" | "models" | "keys" | "appearance" | "server";

export function useSettingsPage() {
  const { connection, setConnection, uiTheme, setUiTheme, locale, setLocale } = useConnectionSettings();
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [draft, setDraft] = useState(connection);
  const [defaultModel, setDefaultModel] = useState<string>("");
  const [desktopNotifications, setDesktopNotifications] = useState(true);
  const [safeFileReads, setSafeFileReads] = useState(true);
  const [safeFileWrites, setSafeFileWrites] = useState(false);
  const [terminalApproval, setTerminalApproval] = useState(true);
  const models = useModelsQuery();

  const modelCards = useMemo(() => models.data ?? [], [models.data]);

  useEffect(() => {
    setDraft(connection);
  }, [connection]);

  useEffect(() => {
    if (!defaultModel && modelCards[0]?.id) {
      setDefaultModel(modelCards[0].id);
    }
  }, [defaultModel, modelCards]);

  return {
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
  };
}

export type SettingsPageState = ReturnType<typeof useSettingsPage>;
