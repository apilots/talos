import { useEffect } from "react";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";

import { APP_ICONS, appIconAssetUrl } from "./catalog";

export function useAppIconEffects() {
  const iconId = useAppStore((state) => state.appIconId);

  useEffect(() => {
    const preset = APP_ICONS[iconId];
    document.head.querySelector("[data-app-icon-favicon]")?.setAttribute("href", appIconAssetUrl(preset.favicon));
    document.head.querySelector("[data-app-icon-touch]")?.setAttribute("href", appIconAssetUrl(preset.appleTouchIcon));
  }, [iconId]);

  useEffect(() => {
    const bridge = getDesktopBridge();
    if (!bridge?.setAppIcon) return;

    let active = true;
    void bridge.setAppIcon(iconId).catch(() => {
      if (active && useAppStore.getState().appIconId === iconId) {
        useAppStore.setState({ appIconDesktopFailed: true });
      }
    });
    return () => { active = false; };
  }, [iconId]);
}
