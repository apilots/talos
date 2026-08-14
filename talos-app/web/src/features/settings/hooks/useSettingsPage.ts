import { useEffect, useState } from "react";

import { useModelsQuery } from "@/features/chat";
import { useConnectionSettings } from "@/features/connection";

export type SettingsSection =
  | "profile"
  | "general"
  | "appearance"
  | "models"
  | "agent-workspace"
  | "mcp-servers"
  | "server";

export const settingsSections: SettingsSection[] = [
  "profile",
  "general",
  "appearance",
  "models",
  "agent-workspace",
  "mcp-servers",
  "server",
];

export function isSettingsSection(section?: string): section is SettingsSection {
  return Boolean(section && settingsSections.includes(section as SettingsSection));
}

export function useSettingsPage() {
  const { connection, setConnection, uiTheme, setUiTheme, locale, setLocale } = useConnectionSettings();
  const [draft, setDraft] = useState(connection);
  const [desktopNotifications, setDesktopNotifications] = useState(true);
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
    models,
  };
}

export type SettingsPageState = ReturnType<typeof useSettingsPage>;
