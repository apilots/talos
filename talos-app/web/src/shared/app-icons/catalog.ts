export {
  APP_ICONS,
  APP_ICON_IDS,
  DEFAULT_APP_ICON_ID,
  resolveAppIconId,
  type AppIconDesktopBridge,
  type AppIconId,
} from "../../../../shared/app-icons";

export function appIconAssetUrl(asset: string): string {
  return `${import.meta.env.BASE_URL}${asset}`;
}
