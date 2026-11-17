export {
  APP_ICONS,
  APP_ICON_IDS,
  DEFAULT_APP_ICON_ID,
  resolveAppIconId,
  type AppIconDesktopBridge,
  type AppIconId,
} from "../../../../shared/app-icons";

export function appIconAssetUrl(id: AppIconId, use: AppIconUse): string {
  return `${import.meta.env.BASE_URL}${appIconAssetPath(id, use)}`;
}
import { appIconAssetPath, type AppIconId, type AppIconUse } from "../../../../shared/app-icons";
