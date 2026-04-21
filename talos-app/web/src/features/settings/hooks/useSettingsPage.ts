import { useEffect, useState } from "react";

import { useModelsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";

export type SettingsTab = "general" | "keys" | "appearance" | "server";

export function useSettingsPage() {
  const { connection, setConnection, uiTheme, setUiTheme, locale, setLocale } = useConnectionSettings();
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [draft, setDraft] = useState(connection);
  const [desktopNotifications, setDesktopNotifications] = useState(true);
  const [safeFileReads, setSafeFileReads] = useState(true);
  const [safeFileWrites, setSafeFileWrites] = useState(false);
  const [terminalApproval, setTerminalApproval] = useState(true);
  const models = useModelsQuery();

  useEffect(() => {
    setDraft(connection);
  }, [connection]);

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
    desktopNotifications,
    setDesktopNotifications,
    safeFileReads,
    setSafeFileReads,
    safeFileWrites,
    setSafeFileWrites,
    terminalApproval,
    setTerminalApproval,
    models,
  };
}

export type SettingsPageState = ReturnType<typeof useSettingsPage>;
