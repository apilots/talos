import { getDesktopBridge } from "@/shared/api/desktop-client";

/** Open an untrusted web destination outside the product renderer. */
export async function openExternalUrl(value: string): Promise<void> {
  const url = new URL(value);
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password) {
    throw new Error("External destinations must be HTTP(S) URLs without credentials.");
  }
  const bridge = getDesktopBridge();
  if (bridge) {
    if (!bridge.openExternal) throw new Error("The Desktop external-browser capability is unavailable.");
    await bridge.openExternal(url.href);
  } else {
    window.open(url.href, "_blank", "noopener,noreferrer");
  }
}
