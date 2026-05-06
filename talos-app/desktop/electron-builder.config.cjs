const path = require("node:path");

const repoRoot = path.resolve(__dirname, "../..");
const packageTarget = process.env.TALOS_DESKTOP_PACKAGE_TARGET ?? "desktop-mac";
const outputDir = process.env.TALOS_DESKTOP_PACKAGE_DIR ?? path.join(repoRoot, `dist/products/dev/${packageTarget}`);
const releaseBuild = process.env.TALOS_DESKTOP_RELEASE === "1";
const macEntitlements = path.join(repoRoot, "build/desktop/mac/entitlements.mac.plist");

module.exports = {
  appId: "ai.talos.desktop",
  productName: "Talos",
  copyright: "Copyright © 2026 Talos",
  directories: {
    output: outputDir,
  },
  files: [
    "dist/**/*",
    "package.json",
    {
      from: "../web/dist",
      to: "web/dist",
      filter: ["**/*"],
    },
  ],
  extraResources: [
    {
      from: "resources/app-server",
      to: "app-server",
      filter: ["**/*"],
    },
  ],
  asar: true,
  mac: {
    icon: path.join(repoRoot, "build/desktop/icons/icon.icns"),
    category: "public.app-category.productivity",
    hardenedRuntime: releaseBuild,
    gatekeeperAssess: false,
    entitlements: releaseBuild ? macEntitlements : undefined,
    entitlementsInherit: releaseBuild ? macEntitlements : undefined,
    notarize: releaseBuild
      ? {
          teamId: process.env.APPLE_TEAM_ID,
        }
      : false,
    target: [
      {
        target: "dmg",
        arch: ["arm64"],
      },
    ],
  },
  dmg: {
    artifactName: "Talos-${version}-${arch}.${ext}",
  },
  linux: {
    icon: path.join(repoRoot, "build/desktop/icons/icon.png"),
    category: "Utility",
    target: ["dir"],
  },
};
