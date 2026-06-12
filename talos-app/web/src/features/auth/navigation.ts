import { getDesktopBridge } from "@/shared/api/desktop-client";

export function redirectToExternalUrl(url: string): void {
  const bridge = getDesktopBridge();
  if (bridge?.openExternal) {
    void bridge.openExternal(url);
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}
