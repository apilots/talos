import { useEffect } from "react";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import { useAppStore } from "@/shared/store/app-store";

import { appIconAssetUrl } from "./catalog";

export function useAppIconEffects() {
  const iconId = useAppStore((state) => state.appIconId);

  useEffect(() => {
    document.head.querySelector("[data-app-icon-favicon]")?.setAttribute("href", appIconAssetUrl(iconId, "favicon"));
    document.head.querySelector("[data-app-icon-touch]")?.setAttribute("href", appIconAssetUrl(iconId, "appleTouchIcon"));
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
