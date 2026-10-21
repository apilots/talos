// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DesktopRendererBridge } from "@talos/sdk-app-server/desktop";

import { getDesktopBridge } from "@/shared/api/desktop-client";
import { openExternalUrl } from "./open-external";

vi.mock("@/shared/api/desktop-client", () => ({ getDesktopBridge: vi.fn() }));
beforeEach(() => { vi.mocked(getDesktopBridge).mockReset(); });
afterEach(() => { vi.restoreAllMocks(); });
it("uses the existing typed Desktop external-browser bridge", async () => {
  const openExternal = vi.fn().mockResolvedValue(undefined);
  vi.mocked(getDesktopBridge).mockReturnValue({ openExternal } as unknown as DesktopRendererBridge);
  const windowOpen = vi.spyOn(window, "open");
  await openExternalUrl("https://talos.love");
  expect(openExternal).toHaveBeenCalledWith("https://talos.love/");
  expect(windowOpen).not.toHaveBeenCalled();
});
it("opens a Web destination without an opener or referrer", async () => {
  const windowOpen = vi.spyOn(window, "open").mockReturnValue(null);
  await openExternalUrl("https://talos.love");
  expect(windowOpen).toHaveBeenCalledWith("https://talos.love/", "_blank", "noopener,noreferrer");
});
it("does not create an Electron child window when Desktop external opening is unavailable", async () => {
  vi.mocked(getDesktopBridge).mockReturnValue({} as DesktopRendererBridge);
  const windowOpen = vi.spyOn(window, "open");
  await expect(openExternalUrl("https://talos.love/")).rejects.toThrow("unavailable");
  expect(windowOpen).not.toHaveBeenCalled();
});
it("never delegates credentialed URLs or non-web schemes", async () => {
  const windowOpen = vi.spyOn(window, "open");
  for (const value of ["javascript:alert(1)", "file:///tmp/test", "https://user:password@example.com"]) {
    await expect(openExternalUrl(value)).rejects.toThrow();
  }
  expect(windowOpen).not.toHaveBeenCalled();
});
