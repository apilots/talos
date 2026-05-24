import { useEffect, useState } from "react";

import { useModelsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";

export type SettingsSection =
  | "general"
  | "models"
  | "agent-workspace"
  | "permissions"
  | "account"
  | "appearance"
  | "advanced";

export const settingsSections: SettingsSection[] = [
  "general",
  "models",
  "agent-workspace",
  "permissions",
  "account",
  "appearance",
  "advanced",
];

export function isSettingsSection(section?: string): section is SettingsSection {
  return Boolean(section && settingsSections.includes(section as SettingsSection));
}

export function useSettingsPage() {
  const { connection, setConnection, uiTheme, setUiTheme, locale, setLocale } = useConnectionSettings();
  const [draft, setDraft] = useState(connection);
  const [desktopNotifications, setDesktopNotifications] = useState(true);
  const [safeFileReads, setSafeFileReads] = useState(true);
  const [safeFileWrites, setSafeFileWrites] = useState(false);
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
    draft,
    setDraft,
    desktopNotifications,
    setDesktopNotifications,
    safeFileReads,
    setSafeFileReads,
    safeFileWrites,
    setSafeFileWrites,
    models,
  };
}

export type SettingsPageState = ReturnType<typeof useSettingsPage>;
