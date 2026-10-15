#!/usr/bin/env bash
set -euo pipefail

# Validate the checked-out release history without network or publication effects.
die() { echo "check-release-version: $*" >&2; exit 1; }
version_files() {
  cat <<'EOF'
deploy/docker/.env.example
deploy/docker/docker-compose.yml
talos-app/desktop/package.json
talos-app/package.json
talos-app/ts-sdk/package.json
talos-app/ts-sdk/packages/app-server/package.json
talos-app/ts-sdk/packages/app-server/src/stdio.ts
talos-app/ts-sdk/packages/browser/package.json
talos-app/ts-sdk/packages/core/package.json
talos-app/web/package.json
talos-rs/Cargo.toml
talos-rs/apps/tui/src/utils.rs
EOF
}
if [[ "${1:-}" == "--files" ]]; then
  version_files
  exit 0
fi
metadata_only=false
if [[ "${1:-}" == "--metadata" ]]; then
  metadata_only=true
  set -- "$(awk '/^\[workspace.package\]/ { active=1; next } /^\[/ { active=0 }
    active && /^version = / { gsub(/\"/, "", $3); print $3; exit }' talos-rs/Cargo.toml)"
fi
VERSION="${1:?release version is required}"
if [[ "${metadata_only}" == true ]]; then
  [[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-SNAPSHOT)?$ ]] || die "expected a Talos version"
else
  [[ "${VERSION}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "expected a formal version"
fi
TAG="v${VERSION}"

assert_file_contains() {
  local file="$1"
  local expected="$2"
  [[ -f "${file}" ]] || die "missing file: ${file}"
  grep -Fq "${expected}" "${file}" || die "${file} does not contain: ${expected}"
}

assert_package_version() {
  local file="$1"
  local expected="$2"
  [[ -f "${file}" ]] || die "missing package file: ${file}"
  PACKAGE_JSON="${file}" EXPECTED_VERSION="${expected}" python3 - <<'PY'
import json
import os
from pathlib import Path

path = Path(os.environ["PACKAGE_JSON"])
expected = os.environ["EXPECTED_VERSION"]
actual = json.loads(path.read_text()).get("version")
if actual != expected:
    raise SystemExit(f"{path} version is {actual!r}, expected {expected!r}")
PY
}

require_version_matches() {
  assert_file_contains "talos-rs/Cargo.toml" "version = \"${VERSION}\""
  assert_file_contains "talos-rs/apps/tui/src/utils.rs" "APP_VERSION: &str = \"${TAG}\""
  assert_package_version "talos-app/package.json" "${VERSION}"
  assert_package_version "talos-app/web/package.json" "${VERSION}"
  assert_package_version "talos-app/desktop/package.json" "${VERSION}"
  assert_package_version "talos-app/ts-sdk/package.json" "${VERSION}"
  assert_package_version "talos-app/ts-sdk/packages/core/package.json" "${VERSION}"
  assert_package_version "talos-app/ts-sdk/packages/browser/package.json" "${VERSION}"
  assert_package_version "talos-app/ts-sdk/packages/app-server/package.json" "${VERSION}"
  assert_file_contains "talos-app/ts-sdk/packages/app-server/src/stdio.ts" \
    "version: this.options.clientVersion ?? \"${VERSION}\""
  local docker_tag="${TAG}"
  if [[ "${metadata_only}" == true && "${VERSION}" == *-SNAPSHOT ]]; then
    docker_tag="$(sed -n 's/^TALOS_SERVER_VERSION=\(v[0-9]*\.[0-9]*\.[0-9]*\)$/\1/p' deploy/docker/.env.example)"
    [[ "${docker_tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Docker default must be a formal version"
  fi
  assert_file_contains "deploy/docker/.env.example" "TALOS_SERVER_VERSION=${docker_tag}"
  assert_file_contains "deploy/docker/docker-compose.yml" "TALOS_SERVER_VERSION:-${docker_tag}"
}

require_version_commit() {
  local expected_subject="chore(release): prepare ${TAG}"
  local version_commit
  version_commit="$(git log --first-parent --format='%H%x09%s' |
    awk -F '\t' -v subject="${expected_subject}" '$2 == subject && !found { print $1; found = 1 }')"
  [[ -n "${version_commit}" ]] \
    || die "release history must contain: ${expected_subject}"

  local expected_files actual_files
  expected_files="$(version_files | sort)"
  actual_files="$(git diff-tree --no-commit-id --name-only -r "${version_commit}" | sort)"
  [[ "${actual_files}" == "${expected_files}" ]] \
    || die "version preparation commit must contain only the required release version files"
  local subject
  local fix_pattern='^fix(\([a-z0-9][a-z0-9._/-]*\))?(!)?: [^[:space:]].*$'
  while IFS= read -r subject; do
    [[ "${subject}" =~ ${fix_pattern} ]] \
      || die "release stabilization only accepts fix commits: ${subject}"
  done < <(git log --format='%s' "${version_commit}..HEAD")
}

require_version_matches
if [[ "${metadata_only}" != true ]]; then
  require_version_commit
fi
