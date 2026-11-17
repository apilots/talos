import catalog from "./catalog.json" with { type: "json" };

export const appIconSizes = { icon: 256, favicon: 96, appleTouchIcon: 180, native: 1024 };

const assetNames = {
  favicon: "favicon.png",
  appleTouchIcon: "apple-touch-icon.png",
  native: "icon.png",
};

export function appIconAssetPath(id, use) {
  const filename = use === "icon"
    ? (catalog.presets[id].format === "svg" ? "icon.svg" : "icon.png")
    : assetNames[use];
  return `app-icons/${id}/${filename}`;
}
