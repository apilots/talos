# App icon sources

Each preset owns its original artwork under its stable ID. Edit `catalog.json`
to register a preset or change the default. The white portrait is the default,
and the independent Browser Workspace home uses the classic vector mark.

The WebUI Vite build generates the in-app, 96 px favicon and 180 px Apple Touch
assets. The Electron build generates the 1024 px native PNGs. Electron Builder
reads the default PNG from the Desktop build output. Both builds use the same
path rule, `app-icons/<id>/<asset>`, in their own output directory.
`generate.mjs <browser-home-output>` also assembles the independent Browser
Workspace home without importing the rasterizer.

Keep preset IDs stable for existing preferences. For new presets, add a
`source.jpg` or `source.svg` in the preset directory and add its localized label
to the settings resources. Generate assets through the normal WebUI or Desktop
build entry points.
