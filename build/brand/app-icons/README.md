# Talos app icon assets

The Portrait dark and Portrait light presets use `portrait/dark.jpg` and
`portrait/light.jpg`, respectively. Their WebUI and Desktop derivatives use
the alpha channel of the original Talos app icon from
`build/brand/talos-app-icon.svg`. The square artwork is fitted proportionally
to the tile's outer bounds before the alpha is applied.

On macOS with `rsvg-convert` and Swift available, regenerate the checked-in
Portrait PNG files from the repository root:

```sh
swift scripts/generate-portrait-app-icons.swift
```

The preset catalog is `talos-app/shared/app-icons.ts`. A bundled preset must
provide an in-app icon, a PNG favicon, an Apple Touch icon, and a native icon.
The renderer persists the preset's ID. Desktop uses the same renderer and
applies the selected native asset through its local bridge. Portrait light is
the default preset and supplies the PNG for Electron Builder's installer icon.
