#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./release.sh <version|vversion> --notes <release-notes.md> [--yes] [--dry-run] [--watch]

Examples:
  ./release.sh 0.1.3 --notes /tmp/talos-v0.1.3.md
  ./release.sh v0.1.3 --notes CHANGELOG.md --watch

This script creates the annotated source tag and canonical GitHub Release.
Artifact publishing is handled by workflows triggered by the source Release.
EOF
}

die() {
  echo "release.sh: $*" >&2
  exit 1
}

run() {
  echo "+ $*"
  if [[ "${DRY_RUN}" != "1" ]]; then
    "$@"
  fi
}

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

normalize_version() {
  local input="$1"
  if [[ "${input}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    VERSION="${input#v}"
    TAG="${input}"
    return
  fi
  if [[ "${input}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    VERSION="${input}"
    TAG="v${input}"
    return
  fi
  die "version must look like 0.1.3 or v0.1.3"
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

require_repo_root() {
  local root
  root="$(git rev-parse --show-toplevel)"
  [[ "${root}" == "${PWD}" ]] || die "run from repository root: ${root}"
}

require_clean_worktree() {
  [[ -z "$(git status --porcelain)" ]] || die "working tree is not clean"
}

require_release_branch() {
  local branch
  branch="$(git branch --show-current)"
  EXPECTED_BRANCH="release-${VERSION}"
  [[ "${branch}" == "${EXPECTED_BRANCH}" ]] \
    || die "release must run from ${EXPECTED_BRANCH}, current branch is ${branch}"
}

require_synced_release_branch() {
  git fetch origin "${EXPECTED_BRANCH}" --tags >/dev/null
  local local_head remote_head
  local_head="$(git rev-parse HEAD)"
  remote_head="$(git rev-parse "origin/${EXPECTED_BRANCH}")"
  [[ "${local_head}" == "${remote_head}" ]] \
    || die "${EXPECTED_BRANCH} is not synced with origin/${EXPECTED_BRANCH}"
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
  assert_file_contains "deploy/docker/.env.example" "TALOS_SERVER_VERSION=${TAG}"
  assert_file_contains "deploy/docker/docker-compose.yml" "TALOS_SERVER_VERSION:-${TAG}"
}

require_version_commit() {
  local expected_subject="chore(release): prepare ${TAG}"
  local actual_subject
  actual_subject="$(git log -1 --format=%s)"
  [[ "${actual_subject}" == "${expected_subject}" ]] \
    || die "HEAD subject must be: ${expected_subject}"

  local expected_files actual_files
  expected_files="$(cat <<'EOF'
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
)"
  actual_files="$(git diff-tree --no-commit-id --name-only -r HEAD | sort)"
  [[ "${actual_files}" == "${expected_files}" ]] \
    || die "HEAD must contain only the required release version files"
}

resolve_tag_action() {
  TAG_ACTION="create"
  if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
    return
  fi

  local tag_sha head_sha
  tag_sha="$(git rev-parse "${TAG}^{commit}")"
  head_sha="$(git rev-parse HEAD)"
  [[ "${tag_sha}" == "${head_sha}" ]] \
    || die "existing tag ${TAG} points to ${tag_sha}, expected ${head_sha}"

  if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
    TAG_ACTION="reuse"
  else
    TAG_ACTION="push"
  fi
}

require_release_absent() {
  if gh release view "${TAG}" >/dev/null 2>&1; then
    die "GitHub Release already exists: ${TAG}"
  fi
}

confirm_release() {
  if [[ "${YES}" == "1" ]]; then
    return
  fi
  echo
  read -r -p "Proceed with tagging ${TAG} and creating the canonical GitHub Release? [y/N] " answer
  [[ "${answer}" == "y" || "${answer}" == "Y" ]] || die "release cancelled"
}

watch_release_runs() {
  echo
  echo "Recent workflow runs for ${TAG}:"
  gh run list \
    --limit 10 \
    --json databaseId,workflowName,event,headBranch,status,conclusion,url \
    --jq ".[] | select(.headBranch == \"${TAG}\") | \"\\(.databaseId)\\t\\(.workflowName)\\t\\(.status)\\t\\(.conclusion // \"\")\\t\\(.url)\""
  if [[ "${WATCH}" == "1" ]]; then
    local run_id
    run_id="$(gh run list --limit 10 --json databaseId,headBranch --jq ".[] | select(.headBranch == \"${TAG}\") | .databaseId" | head -n 1)"
    [[ -n "${run_id}" ]] || die "no workflow run found for ${TAG}"
    gh run watch "${run_id}"
  fi
}

VERSION=""
TAG=""
EXPECTED_BRANCH=""
TAG_ACTION=""
NOTES_FILE=""
YES=0
DRY_RUN=0
WATCH=0

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

normalize_version "$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --notes)
      [[ $# -ge 2 ]] || die "--notes requires a file path"
      NOTES_FILE="$2"
      shift 2
      ;;
    --yes)
      YES=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --watch)
      WATCH=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ -n "${NOTES_FILE}" ]] || die "--notes is required"
[[ -f "${NOTES_FILE}" ]] || die "release notes file not found: ${NOTES_FILE}"
[[ -s "${NOTES_FILE}" ]] || die "release notes file is empty: ${NOTES_FILE}"
[[ "$(head -n 1 "${NOTES_FILE}")" == "# Talos ${TAG}" ]] \
  || die "release notes must start with the heading: # Talos ${TAG}"

require_command git
require_command gh
require_command python3
require_repo_root
require_clean_worktree
require_release_branch
require_synced_release_branch
gh auth status >/dev/null
require_version_matches
require_version_commit
resolve_tag_action
require_release_absent

HEAD_SHA="$(git rev-parse HEAD)"

cat <<EOF
Release summary:
  version:    ${VERSION}
  tag:        ${TAG}
  branch:     ${EXPECTED_BRANCH}
  commit:     ${HEAD_SHA}
  notes file: ${NOTES_FILE}
  tag action: ${TAG_ACTION}
  dry run:    ${DRY_RUN}
EOF

confirm_release

if [[ "${TAG_ACTION}" == "create" ]]; then
  run git tag -a "${TAG}" -m "Release ${TAG}"
fi
if [[ "${TAG_ACTION}" != "reuse" ]]; then
  run git push origin "${TAG}"
else
  echo "+ reuse existing remote tag ${TAG} at ${HEAD_SHA}"
fi
run gh release create "${TAG}" --title "Talos ${TAG}" --notes-file "${NOTES_FILE}"

if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Dry run complete."
  exit 0
fi

echo
echo "Release created:"
gh release view "${TAG}" --json tagName,name,url,isDraft,isPrerelease,publishedAt
watch_release_runs
