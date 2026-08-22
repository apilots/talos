#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_svg="$repo_root/build/brand/talos-logo.svg"
app_icon_svg="$repo_root/build/brand/talos-app-icon.svg"
web_public="$repo_root/talos-app/web/public"
desktop_icons="$repo_root/build/desktop/icons"

if ! command -v rsvg-convert >/dev/null 2>&1; then
  echo "rsvg-convert is required to generate logo assets." >&2
  exit 127
fi

if ! command -v iconutil >/dev/null 2>&1; then
  echo "iconutil is required to generate the macOS ICNS asset." >&2
  exit 127
fi

if grep -Eq '<image|data:image' "$source_svg"; then
  echo "The logo master must not embed raster images." >&2
  exit 1
fi

node - "$source_svg" "$app_icon_svg" <<'NODE'
const fs = require("node:fs");

const [sourcePath, outputPath] = process.argv.slice(2);
const source = fs.readFileSync(sourcePath, "utf8");
const innerSvg = source
  .replace(/^<svg[^>]*>\s*/, "")
  .replace(/\s*<\/svg>\s*$/, "");
const output = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1254 1254" role="img" aria-labelledby="app-icon-title app-icon-description">
  <title id="app-icon-title">Talos app icon</title>
  <desc id="app-icon-description">The Talos mark centered on a white application icon tile.</desc>
  <rect x="58" y="58" width="1138" height="1138" rx="254" fill="#ffffff" stroke="#dfe4ec" stroke-width="8"/>
  <g transform="translate(152 152) scale(0.7575757576)">
${innerSvg}
  </g>
</svg>
`;

fs.writeFileSync(outputPath, output);
NODE

render_png() {
  local size="$1"
  local output="$2"
  rsvg-convert \
    --keep-aspect-ratio \
    --width "$size" \
    --height "$size" \
    --output "$output" \
    "$source_svg"
}

render_app_icon_png() {
  local size="$1"
  local output="$2"
  rsvg-convert \
    --keep-aspect-ratio \
    --width "$size" \
    --height "$size" \
    --output "$output" \
    "$app_icon_svg"
}

render_touch_icon_png() {
  local size="$1"
  local output="$2"
  rsvg-convert \
    --background-color "#ffffff" \
    --keep-aspect-ratio \
    --width "$size" \
    --height "$size" \
    --output "$output" \
    "$app_icon_svg"
}

cp "$source_svg" "$web_public/talos-logo.svg"
cp "$app_icon_svg" "$web_public/app-icon.svg"
cp "$app_icon_svg" "$desktop_icons/talos-icon.svg"

render_png 1254 "$repo_root/build/brand/talos-logo.png"
render_png 1254 "$web_public/logo.png"
render_app_icon_png 96 "$web_public/favicon.png"
render_touch_icon_png 180 "$web_public/apple-touch-icon.png"
render_app_icon_png 1024 "$desktop_icons/icon.png"

iconset_dir="$(mktemp -d /tmp/talos-logo-iconset.XXXXXX)/Talos.iconset"
mkdir -p "$iconset_dir"
trap 'rm -rf "${iconset_dir%/Talos.iconset}"' EXIT

render_app_icon_png 16 "$iconset_dir/icon_16x16.png"
render_app_icon_png 32 "$iconset_dir/icon_16x16@2x.png"
render_app_icon_png 32 "$iconset_dir/icon_32x32.png"
render_app_icon_png 64 "$iconset_dir/icon_32x32@2x.png"
render_app_icon_png 128 "$iconset_dir/icon_128x128.png"
render_app_icon_png 256 "$iconset_dir/icon_128x128@2x.png"
render_app_icon_png 256 "$iconset_dir/icon_256x256.png"
render_app_icon_png 512 "$iconset_dir/icon_256x256@2x.png"
render_app_icon_png 512 "$iconset_dir/icon_512x512.png"
render_app_icon_png 1024 "$iconset_dir/icon_512x512@2x.png"

iconutil --convert icns --output "$desktop_icons/icon.icns" "$iconset_dir"

echo "Generated Talos brand and application icon assets from $source_svg"
