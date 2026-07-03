import { getDesktopBridge } from "@/shared/api/desktop-client";

export type PreparedExternalWindow = Window | null;

export function prepareExternalAuthWindow(): PreparedExternalWindow {
  const bridge = getDesktopBridge();
  if (bridge?.openExternal) {
    return null;
  }
  return window.open("about:blank", "_blank");
}

export function redirectToExternalUrl(url: string, preparedWindow?: PreparedExternalWindow): void {
  const bridge = getDesktopBridge();
  if (bridge?.openExternal) {
    void bridge.openExternal(url);
    return;
  }
  if (preparedWindow && !preparedWindow.closed) {
    preparedWindow.location.href = url;
    preparedWindow.opener = null;
    return;
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

export function closePreparedExternalWindow(preparedWindow?: PreparedExternalWindow): void {
  if (preparedWindow && !preparedWindow.closed) {
    preparedWindow.close();
  }
}
