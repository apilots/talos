#!/usr/bin/env bash
# Build-only supplier shared by the Server and Worker runtime images.
set -euo pipefail

export PLAYWRIGHT_BROWSERS_PATH=/opt/talos/browser/engines
npm install --prefix /build/playwright --ignore-scripts playwright@1.64.1
node /build/playwright/node_modules/playwright/cli.js install --no-shell chromium
node <<'JS'
const fs = require('node:fs');
const playwright = require('/build/playwright/node_modules/playwright');
fs.writeFileSync('/opt/talos/browser/runtime.json', JSON.stringify({
  schemaVersion: 1,
  browserUseVersion: '',
  chromiumVersion: '156.0.8078.4',
  python: '/opt/talos/browser/venv/bin/python',
  driver: '/opt/talos/browser/browser_driver.py',
  chromium: playwright.chromium.executablePath(),
}));
JS
chmod -R a+rX /opt/talos/browser/engines
