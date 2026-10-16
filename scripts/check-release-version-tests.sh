#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
validator="${repo_root}/scripts/check-release-version.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

git -C "${temp_dir}" init -q -b release-1.2.3
git -C "${temp_dir}" config user.name "Release Test"
git -C "${temp_dir}" config user.email "release-test@example.com"
git -C "${temp_dir}" -c core.hooksPath=/dev/null commit -q --allow-empty -m "chore: initialize"
cd "${temp_dir}"
python3 - <<'PY'
from pathlib import Path

files = {
    "talos-rs/Cargo.toml": '[workspace.package]\nversion = "1.2.3"\n',
    "talos-rs/apps/tui/src/utils.rs": 'pub const APP_VERSION: &str = "v1.2.3";\n',
    "talos-app/ts-sdk/packages/app-server/src/stdio.ts": 'version: this.options.clientVersion ?? "1.2.3"\n',
    "deploy/docker/talos-worker/.env.example": 'TALOS_WORKER_VERSION=v1.2.3\n',
    "deploy/docker/talos-server/.env.example": 'TALOS_SERVER_VERSION=v1.2.3\n',
    "deploy/docker/talos-server/docker-compose.yml": 'image: ghcr.io/apilots/talos-server:${TALOS_SERVER_VERSION:-v1.2.3}\n',
}
for package in ("", "web/", "desktop/", "ts-sdk/", "ts-sdk/packages/core/",
                "ts-sdk/packages/browser/", "ts-sdk/packages/app-server/"):
    files[f"talos-app/{package}package.json"] = '{"version":"1.2.3"}\n'
for name, text in files.items():
    path = Path(name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
PY

git add .
git -c core.hooksPath=/dev/null commit -q -m "chore(release): prepare v1.2.3"
bash "${validator}" 1.2.3
printf 'repair\n' > bugfix
git add bugfix
git -c core.hooksPath=/dev/null commit -q -m "fix(release): repair chore(release): prepare v1.2.3 validation"
bash "${validator}" 1.2.3

expect_rejection() {
  local reason="$1"
  if bash "${validator}" 1.2.3 >validation.log 2>&1; then
    echo "Expected rejection: ${reason}" >&2
    exit 1
  fi
  grep -Fq "${reason}" validation.log
}

git -c core.hooksPath=/dev/null commit -q --allow-empty -m "feat: add feature"
expect_rejection "release stabilization only accepts fix commits"
git reset --hard -q HEAD^
for subject in 'fix(/): invalid scope' 'fix: '; do
  git -c core.hooksPath=/dev/null commit -q --allow-empty -m "${subject}"
  expect_rejection "release stabilization only accepts fix commits"
  git reset --hard -q HEAD^
done
printf '{"version":"1.2.4-SNAPSHOT"}\n' > talos-app/package.json
expect_rejection "expected '1.2.3'"
git restore talos-app/package.json
git reset --soft HEAD~2
git -c core.hooksPath=/dev/null commit -q -m "chore(release): prepare v1.2.3"
expect_rejection "version preparation commit must contain only"

# Main closeout validates consistent snapshot metadata without release-only history.
git reset --hard -q
python3 - <<'PY_METADATA'
from pathlib import Path
for root in [Path("talos-app"), Path("talos-rs")]:
    for path in root.rglob("*"):
        if path.is_file():
            path.write_text(path.read_text().replace("1.2.3", "1.2.4-SNAPSHOT"))
PY_METADATA
bash "${validator}" --metadata
printf '{"version":"1.2.5-SNAPSHOT"}\n' > talos-app/package.json
if bash "${validator}" --metadata >metadata.log 2>&1; then
  echo "Expected mismatched snapshot metadata to fail" >&2
  exit 1
fi
grep -Fq "expected '1.2.4-SNAPSHOT'" metadata.log

echo "Release version lifecycle tests passed."
