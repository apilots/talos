const path = require("node:path");

const repoRoot = path.resolve(__dirname, "../..");
const packageTarget = process.env.TALOS_DESKTOP_PACKAGE_TARGET ?? "desktop-mac";
const outputDir = process.env.TALOS_DESKTOP_PACKAGE_DIR ?? path.join(repoRoot, `dist/products/dev/${packageTarget}`);
const releaseBuild = process.env.TALOS_DESKTOP_RELEASE === "1";
const macEntitlements = path.join(repoRoot, "build/desktop/mac/entitlements.mac.plist");
const dmgBackground = path.join(repoRoot, "build/desktop/dmg/background.png");
const packageArch = process.env.TALOS_PACKAGE_ARCH ?? (process.arch === "arm64" ? "arm64" : "amd64");
const electronBuilderArch = packageArch === "arm64" ? "arm64" : "x64";

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
    {
      from: "resources/conf",
      to: "conf",
      filter: ["**/*"],
    },
    {
      from: path.join(repoRoot, "LICENSE"),
      to: "licenses/LICENSE",
    },
    {
      from: path.join(repoRoot, "NOTICE"),
      to: "licenses/NOTICE",
    },
    {
      from: path.join(repoRoot, "THIRD_PARTY_NOTICES"),
      to: "licenses/THIRD_PARTY_NOTICES",
    },
  ],
  asar: true,
  // Native application tools execute package resources and require a real cwd.
  asarUnpack: ["node_modules/**/*"],
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
    artifactName: "Talos-Desktop-${version}-macos-${arch}.${ext}",
    background: dmgBackground,
    contents: [
      {
        x: 140,
        y: 205,
        type: "file",
      },
      {
        x: 400,
        y: 205,
        type: "link",
        path: "/Applications",
      },
    ],
    iconSize: 108,
    iconTextSize: 12,
    title: "Talos",
    window: {
      width: 540,
      height: 380,
    },
  },
  linux: {
    icon: path.join(repoRoot, "build/desktop/icons/icon.png"),
    category: "Utility",
    artifactName: `Talos-Desktop-\${version}-linux-${packageArch}.\${ext}`,
    target: [
      {
        target: "AppImage",
        arch: [electronBuilderArch],
      },
    ],
  },
};
