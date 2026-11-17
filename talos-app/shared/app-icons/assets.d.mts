import type { AppIconId } from "./index";

export type AppIconUse = "icon" | "favicon" | "appleTouchIcon" | "native";

export const appIconSizes: Readonly<Record<AppIconUse, number>>;
export function appIconAssetPath(id: AppIconId, use: AppIconUse): string;
