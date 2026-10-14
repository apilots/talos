#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./release.sh <version|vversion> [--notes <release-notes.md>] [--yes] [--dry-run] [--watch]

Examples:
  ./release.sh 0.1.10 --dry-run --yes
  ./release.sh v0.1.10 --notes output/release-notes/v0.1.10.md --watch

This script creates the annotated source tag with canonical release notes.
Notes default to the successful candidate's standard generator output.
The tag workflow reuses that source gate, then builds and verifies formal artifacts.
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

require_tag_annotation() {
  [[ "$(git cat-file -t "${TAG}")" == "tag" ]] \
    || die "${TAG} must be an annotated tag"
  local contents
  contents="$(git for-each-ref --format='%(contents)' "refs/tags/${TAG}")"
  [[ "${contents%%$'\n'*}" == "# Talos ${TAG}" ]] \
    || die "tag annotation must start with: # Talos ${TAG}"
  git cat-file tag "${TAG}" | sed '1,/^$/d' > "${CANDIDATE_DIR}/tag-notes.md"
  cmp "${NOTES_FILE}" "${CANDIDATE_DIR}/tag-notes.md" \
    || die "tag annotation must equal the candidate's canonical Notes"
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

  require_tag_annotation

  if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
    TAG_ACTION="reuse"
  else
    TAG_ACTION="push"
  fi
}

require_release_absent() {
  local query_error="${CANDIDATE_DIR}/release-query-error"
  if gh api "repos/${REPOSITORY}/releases/tags/${TAG}" >/dev/null 2>"${query_error}"; then
    die "GitHub Release already exists: ${TAG}"
  fi
  if ! grep -Fq 'HTTP 404' "${query_error}"; then
    cat "${query_error}" >&2
    die "could not verify release absence"
  fi
}

require_validated_candidate() {
  CANDIDATE_RUN="$(python3 .github/scripts/release_candidate.py find \
    --repository "${REPOSITORY}" --sha "${HEAD_SHA}" \
    --outputs "${CANDIDATE_DIR}/candidate.outputs")"
  local artifact_name
  artifact_name="$(sed -n 's/^artifact_name=//p' "${CANDIDATE_DIR}/candidate.outputs")"
  gh run download "${CANDIDATE_RUN}" --repo "${REPOSITORY}" \
    --name "${artifact_name}" --dir "${CANDIDATE_DIR}"
  python3 .github/scripts/release_candidate.py verify \
    --repository "${REPOSITORY}" --sha "${HEAD_SHA}" \
    --run-id "${CANDIDATE_RUN}" --directory "${CANDIDATE_DIR}"
  if [[ -z "${NOTES_FILE}" ]]; then
    mkdir -p output/release-notes
    NOTES_FILE="${PWD}/output/release-notes/${TAG}.md"
    cp "${CANDIDATE_DIR}/release-body.md" "${NOTES_FILE}"
  else
    cmp "${NOTES_FILE}" "${CANDIDATE_DIR}/release-body.md" \
      || die "--notes must equal the candidate's standard generator output"
  fi
}

confirm_release() {
  if [[ "${YES}" == "1" ]]; then
    return
  fi
  echo
  read -r -p "Proceed with tagging ${TAG} and starting the release workflow? [y/N] " answer
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

if [[ -n "${NOTES_FILE}" ]]; then
  [[ -f "${NOTES_FILE}" ]] || die "release notes file not found: ${NOTES_FILE}"
fi

require_command git
require_command gh
require_command python3
require_repo_root
require_clean_worktree
require_release_branch
require_synced_release_branch
gh auth status >/dev/null
bash scripts/check-release-version.sh "${VERSION}"
HEAD_SHA="$(git rev-parse HEAD)"
REPOSITORY="$(gh repo view --json nameWithOwner --jq '.nameWithOwner')"
mkdir -p output
CANDIDATE_DIR="$(mktemp -d "${PWD}/output/release-candidate.XXXXXX")"
trap 'rm -rf "${CANDIDATE_DIR}"' EXIT
require_release_absent
require_validated_candidate
resolve_tag_action

cat <<EOF
Release summary:
  version:    ${VERSION}
  tag:        ${TAG}
  branch:     ${EXPECTED_BRANCH}
  commit:     ${HEAD_SHA}
  candidate:  ${CANDIDATE_RUN}
  notes file: ${NOTES_FILE}
  tag action: ${TAG_ACTION}
  dry run:    ${DRY_RUN}
EOF

confirm_release

if [[ "${TAG_ACTION}" == "create" ]]; then
  run git tag -a --cleanup=verbatim "${TAG}" -F "${NOTES_FILE}"
  if [[ "${DRY_RUN}" != "1" ]]; then
    require_tag_annotation
  fi
fi
if [[ "${TAG_ACTION}" != "reuse" ]]; then
  run git push origin "${TAG}"
else
  echo "+ reuse existing remote tag ${TAG} at ${HEAD_SHA}"
fi
if [[ "${DRY_RUN}" == "1" ]]; then
  echo "Dry run complete."
  exit 0
fi

echo
echo "Release workflow started by pushing ${TAG}."
watch_release_runs
