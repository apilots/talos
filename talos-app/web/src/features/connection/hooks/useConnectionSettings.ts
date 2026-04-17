import { useAppStore } from "@/shared/store/app-store";

export function useConnectionSettings() {
  const connection = useAppStore((state) => state.connection);
  const setConnection = useAppStore((state) => state.setConnection);
  const uiTheme = useAppStore((state) => state.uiTheme);
  const setUiTheme = useAppStore((state) => state.setUiTheme);
  const locale = useAppStore((state) => state.locale);
  const setLocale = useAppStore((state) => state.setLocale);

  return {
    connection,
    setConnection,
    uiTheme,
    setUiTheme,
    locale,
    setLocale,
  };
}
