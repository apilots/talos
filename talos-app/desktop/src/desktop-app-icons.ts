import path from "node:path";
import { app, nativeImage, type NativeImage } from "electron";

import { appIconAssetPath, type AppIconId } from "../../shared/app-icons";
import { desktopWindows } from "./desktop-window";

export class DesktopAppIcons {
  private linuxWindowIcon?: NativeImage;

  constructor(
    private readonly assetDirectory: string,
    private readonly platform: NodeJS.Platform = process.platform,
  ) {}

  get windowIcon(): NativeImage | undefined {
    return this.linuxWindowIcon;
  }

  setAppIcon(id: AppIconId): void {
    if (this.platform !== "darwin" && this.platform !== "linux") {
      return;
    }

    const filename = appIconAssetPath(id, "native");
    const image = nativeImage.createFromPath(path.join(this.assetDirectory, filename));
    if (image.isEmpty()) {
      throw new Error(`Unable to decode the bundled app icon: ${filename}`);
    }

    if (this.platform === "darwin") {
      const dock = app.dock;
      if (!dock) {
        throw new Error("The macOS Dock is unavailable.");
      }
      dock.setIcon(image);
      return;
    }

    for (const { window } of desktopWindows) {
      if (!window.isDestroyed()) {
        window.setIcon(image);
      }
    }
    this.linuxWindowIcon = image;
  }
}
