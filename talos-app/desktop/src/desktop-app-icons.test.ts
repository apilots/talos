import path from "node:path";
import type { BaseWindow, WebContentsView } from "electron";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { appIconAssetPath, APP_ICON_IDS, DEFAULT_APP_ICON_ID } from "../../shared/app-icons";

const fixture = vi.hoisted(() => ({
  setDockIcon: vi.fn(),
  createFromPath: vi.fn(),
}));
vi.mock("electron", () => ({
  app: { dock: { setIcon: fixture.setDockIcon } },
  nativeImage: { createFromPath: fixture.createFromPath },
}));

import { DesktopAppIcons } from "./desktop-app-icons";
import { desktopWindows } from "./desktop-window";

const images = new Map<string, { isEmpty: () => boolean }>(
  APP_ICON_IDS.map(id => [id, { isEmpty: () => false }]),
);
const assetDirectory = path.resolve("app-icon-test-assets");
const imagesByPath = new Map(APP_ICON_IDS.map(id => [
  path.join(assetDirectory, appIconAssetPath(id, "native")),
  images.get(id),
]));

beforeEach(() => {
  vi.clearAllMocks();
  fixture.createFromPath.mockImplementation((filename: string) => imagesByPath.get(filename));
});

afterEach(() => {
  desktopWindows.clear();
});

function addProductWindow(destroyed = false) {
  const window = { isDestroyed: () => destroyed, setIcon: vi.fn() };
  desktopWindows.add({ window: window as unknown as BaseWindow, view: {} as WebContentsView });
  return window;
}

it("updates the macOS Dock and restores the bundled portrait default", () => {
  const icons = new DesktopAppIcons(assetDirectory, "darwin");
  const window = addProductWindow();
  icons.setAppIcon("portrait");
  icons.setAppIcon("classic");
  icons.setAppIcon(DEFAULT_APP_ICON_ID);
  expect(fixture.setDockIcon).toHaveBeenNthCalledWith(1, images.get("portrait"));
  expect(fixture.setDockIcon).toHaveBeenNthCalledWith(2, images.get("classic"));
  expect(fixture.setDockIcon).toHaveBeenNthCalledWith(3, images.get("portrait-light"));
  expect(window.setIcon).not.toHaveBeenCalled();
});

it("updates live Linux product windows and retains their latest icon for opening new windows", () => {
  const icons = new DesktopAppIcons(assetDirectory, "linux");
  const live = addProductWindow();
  const destroyed = addProductWindow(true);

  expect(icons.windowIcon).toBeUndefined();
  icons.setAppIcon("portrait-light");
  expect(live.setIcon).toHaveBeenCalledExactlyOnceWith(images.get("portrait-light"));
  expect(destroyed.setIcon).not.toHaveBeenCalled();
  expect(icons.windowIcon).toBe(images.get("portrait-light"));
  icons.setAppIcon("classic");
  expect(live.setIcon).toHaveBeenLastCalledWith(images.get("classic"));
  expect(icons.windowIcon).toBe(images.get("classic"));
  expect(fixture.setDockIcon).not.toHaveBeenCalled();
});

it("retains the current icon when Electron cannot load a bundled replacement", () => {
  const icons = new DesktopAppIcons(assetDirectory, "linux");
  const window = addProductWindow();
  icons.setAppIcon("classic");

  fixture.createFromPath.mockReturnValueOnce({ isEmpty: () => true });
  expect(() => icons.setAppIcon("portrait-light")).toThrow("Unable to decode the bundled app icon");
  expect(window.setIcon).toHaveBeenCalledExactlyOnceWith(images.get("classic"));
  expect(icons.windowIcon).toBe(images.get("classic"));
});
