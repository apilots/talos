import { copyFile, cp, mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { appIconAssetPath, appIconSizes } from "./assets.mjs";
import catalog from "./catalog.json" with { type: "json" };

const viewSize = 1254;
const inset = 58;
const border = 8;
const side = viewSize - 2 * inset;
const tile = `<rect x="${inset}" y="${inset}" width="${side}" height="${side}" rx="254" fill="#ffffff" stroke="#dfe4ec" stroke-width="${border}"/>`;

function iconSource(id) {
  return new URL(`${id}/source.${catalog.presets[id].format}`, import.meta.url);
}

function tileSvg(mark = "", size = viewSize) {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${viewSize} ${viewSize}" width="${size}" height="${size}">${tile}${mark}</svg>`;
}

export async function generateAppIcons(target, output) {
  const { createCanvas, loadImage } = await import("@napi-rs/canvas");
  const uses = target === "web" ? ["icon", "favicon", "appleTouchIcon"] : ["native"];
  const masks = new Map();
  for (const use of uses) {
    const size = appIconSizes[use];
    const ctx = createCanvas(size, size).getContext("2d");
    ctx.drawImage(await loadImage(Buffer.from(tileSvg("", size))), 0, 0);
    masks.set(size, ctx.getImageData(0, 0, size, size).data);
  }

  for (const [id, preset] of Object.entries(catalog.presets)) {
    const source = await readFile(iconSource(id));
    const portrait = preset.format === "jpg" ? await loadImage(source) : null;
    if (portrait && portrait.width !== portrait.height) {
      throw new Error(`App icon source must be square: ${id}`);
    }
    const mark = portrait ? "" : `<g transform="translate(152 152) scale(0.7575757576)">${source.toString()
      .replace(/^<svg[^>]*>\s*/, "").replace(/\s*<\/svg>\s*$/, "")}</g>`;

    for (const use of uses) {
      const size = appIconSizes[use];
      const destination = path.join(output, appIconAssetPath(id, use));
      await mkdir(path.dirname(destination), { recursive: true });
      if (use === "icon" && !portrait) {
        await writeFile(destination, tileSvg(mark));
        continue;
      }

      const canvas = createCanvas(size, size);
      const ctx = canvas.getContext("2d");
      if (portrait) {
        // Fit the square to the tile's outer stroke bounds, then share its alpha.
        const padding = (inset - border / 2) * size / viewSize;
        const width = size - 2 * padding;
        ctx.drawImage(portrait, padding, padding, width, width);
        const pixels = ctx.getImageData(0, 0, size, size);
        const mask = masks.get(size);
        for (let i = 3; i < mask.length; i += 4) pixels.data[i] = mask[i];
        ctx.putImageData(pixels, 0, 0);
      } else {
        ctx.drawImage(await loadImage(Buffer.from(tileSvg(mark, size))), 0, 0);
      }
      await writeFile(destination, canvas.toBuffer("image/png"));
    }
  }
}

export async function prepareBrowserHome(output) {
  if (catalog.presets[catalog.browserHomeId]?.format !== "svg") {
    throw new Error("The Browser Workspace home requires an SVG icon source");
  }
  await cp(new URL("../browser-home/", import.meta.url), output, { recursive: true });
  await copyFile(iconSource(catalog.browserHomeId), path.join(output, "talos-logo.svg"));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.length !== 3) throw new Error("Usage: node generate.mjs <browser-home-output>");
  await prepareBrowserHome(process.argv[2]);
}
