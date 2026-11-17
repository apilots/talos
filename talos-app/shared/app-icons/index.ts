import catalog from "./catalog.json";

export { appIconAssetPath, type AppIconUse } from "./assets.mjs";

export const APP_ICONS = catalog.presets;
export type AppIconId = keyof typeof APP_ICONS;
export const APP_ICON_IDS = Object.keys(APP_ICONS) as readonly AppIconId[];

export function isAppIconId(value: unknown): value is AppIconId {
  return typeof value === "string" && Object.hasOwn(APP_ICONS, value);
}

if (!isAppIconId(catalog.defaultId)) throw new Error("Unknown default app icon");
export const DEFAULT_APP_ICON_ID: AppIconId = catalog.defaultId;

export function resolveAppIconId(value: unknown): AppIconId {
  return isAppIconId(value) ? value : DEFAULT_APP_ICON_ID;
}

export interface AppIconDesktopBridge {
  setAppIcon(id: AppIconId): Promise<void>;
}
