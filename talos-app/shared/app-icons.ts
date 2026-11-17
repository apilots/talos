export const APP_ICONS = {
  "portrait-light": {
    labelKey: "appIcons.portraitLight",
    icon: "app-icons/portrait-light/icon.png",
    favicon: "app-icons/portrait-light/favicon.png",
    appleTouchIcon: "app-icons/portrait-light/apple-touch-icon.png",
    nativeFile: "portrait-light.png",
  },
  portrait: {
    labelKey: "appIcons.portrait",
    icon: "app-icons/portrait/icon.png",
    favicon: "app-icons/portrait/favicon.png",
    appleTouchIcon: "app-icons/portrait/apple-touch-icon.png",
    nativeFile: "portrait.png",
  },
  classic: {
    labelKey: "appIcons.classic",
    icon: "app-icon.svg",
    favicon: "favicon.png",
    appleTouchIcon: "apple-touch-icon.png",
    nativeFile: "icon.png",
  },
} as const;

export type AppIconId = keyof typeof APP_ICONS;

export const DEFAULT_APP_ICON_ID: AppIconId = "portrait-light";
export const APP_ICON_IDS = Object.keys(APP_ICONS) as readonly AppIconId[];

export function isAppIconId(value: unknown): value is AppIconId {
  return typeof value === "string" && Object.hasOwn(APP_ICONS, value);
}

export function resolveAppIconId(value: unknown): AppIconId {
  return isAppIconId(value) ? value : DEFAULT_APP_ICON_ID;
}

export interface AppIconDesktopBridge {
  setAppIcon(id: AppIconId): Promise<void>;
}
