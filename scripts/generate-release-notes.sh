#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/commit-types.sh"

usage() {
  cat <<'EOF'
Usage:
  scripts/generate-release-notes.sh <version|vversion> --from <ref> [--to <ref>] [--output <file>]

Examples:
  scripts/generate-release-notes.sh v0.1.7 --from v0.1.6 --to ae00bfd9 --output /tmp/talos-v0.1.7.md
  scripts/generate-release-notes.sh 0.1.8 --from v0.1.7 --output /tmp/talos-v0.1.8.md
EOF
}

die() {
  echo "generate-release-notes.sh: $*" >&2
  exit 1
}

normalize_version() {
  local input="$1"
  if [[ "${input}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    TAG="${input}"
  elif [[ "${input}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    TAG="v${input}"
  else
    die "version must look like 0.1.8 or v0.1.8"
  fi
}

append_entry() {
  local file="$1"
  local scope="$2"
  local breaking="$3"
  local description="$4"
  local prefix=""

  if [[ -n "${breaking}" ]]; then
    prefix="**Breaking:** "
  fi
  if [[ -n "${scope}" ]]; then
    printf -- '- %s**%s:** %s\n' "${prefix}" "${scope}" "${description}" >> "${file}"
  else
    printf -- '- %s%s\n' "${prefix}" "${description}" >> "${file}"
  fi
}

emit_section() {
  local title="$1"
  local file="$2"
  if [[ ! -s "${file}" ]]; then
    return 0
  fi
  printf '## %s\n\n' "${title}"
  cat "${file}"
  printf '\n'
}

TAG=""
FROM_REF=""
TO_REF="HEAD"
OUTPUT_FILE=""
SOURCE_REPOSITORY="${SOURCE_REPOSITORY:-apilots/talos}"

[[ $# -gt 0 ]] || {
  usage
  exit 1
}
normalize_version "$1"
shift

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from)
      [[ $# -ge 2 ]] || die "--from requires a ref"
      FROM_REF="$2"
      shift 2
      ;;
    --to)
      [[ $# -ge 2 ]] || die "--to requires a ref"
      TO_REF="$2"
      shift 2
      ;;
    --output)
      [[ $# -ge 2 ]] || die "--output requires a file"
      OUTPUT_FILE="$2"
      shift 2
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

[[ -n "${FROM_REF}" ]] || die "--from is required"
git rev-parse --verify "${FROM_REF}^{commit}" >/dev/null 2>&1 || die "unknown from ref: ${FROM_REF}"
git rev-parse --verify "${TO_REF}^{commit}" >/dev/null 2>&1 || die "unknown to ref: ${TO_REF}"
git merge-base --is-ancestor "${FROM_REF}" "${TO_REF}" || die "${FROM_REF} is not an ancestor of ${TO_REF}"

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

features_file="${temp_dir}/features"
fixes_file="${temp_dir}/fixes"
performance_file="${temp_dir}/performance"
improvements_file="${temp_dir}/improvements"
documentation_file="${temp_dir}/documentation"
build_ci_file="${temp_dir}/build-ci"
tests_file="${temp_dir}/tests"
tooling_file="${temp_dir}/tooling"
maintenance_file="${temp_dir}/maintenance"
reverts_file="${temp_dir}/reverts"
other_file="${temp_dir}/other"
touch "${features_file}" "${fixes_file}" "${performance_file}" "${improvements_file}" \
  "${documentation_file}" "${build_ci_file}" "${tests_file}" "${tooling_file}" \
  "${maintenance_file}" "${reverts_file}" "${other_file}"

commit_pattern="^(${COMMIT_TYPES_PATTERN})(\(([^)]+)\))?(!)?:[[:space:]]*(.+)$"
typed_pattern='^[a-z][a-z0-9-]*(\([^)]*\))?(!)?:'
entry_count=0

while IFS= read -r subject || [[ -n "${subject}" ]]; do
  [[ -n "${subject}" ]] || continue
  if [[ "${subject}" =~ ^Release[[:space:]]+(Talos[[:space:]]+)?v?[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    continue
  fi
  if [[ "${subject}" =~ ^chore\(release\):[[:space:]]+(prepare[[:space:]]+v?[0-9]+\.[0-9]+\.[0-9]+|start[[:space:]]+v?[0-9]+\.[0-9]+\.[0-9]+[[:space:]]+development)$ ]]; then
    continue
  fi

  if [[ "${subject}" =~ ${commit_pattern} ]]; then
    type="${BASH_REMATCH[1]}"
    scope="${BASH_REMATCH[3]}"
    breaking="${BASH_REMATCH[4]}"
    description="${BASH_REMATCH[5]}"
    case "${type}" in
      feat) target="${features_file}" ;;
      fix) target="${fixes_file}" ;;
      perf) target="${performance_file}" ;;
      refactor) target="${improvements_file}" ;;
      docs) target="${documentation_file}" ;;
      build|ci) target="${build_ci_file}" ;;
      test) target="${tests_file}" ;;
      tool) target="${tooling_file}" ;;
      chore)
        if [[ "${scope}" == "ci" || "${scope}" == "build" ]]; then
          target="${build_ci_file}"
        else
          target="${maintenance_file}"
        fi
        ;;
      style) target="${maintenance_file}" ;;
      revert) target="${reverts_file}" ;;
    esac
    append_entry "${target}" "${scope}" "${breaking}" "${description}"
  elif [[ "${subject}" =~ ${typed_pattern} ]]; then
    die "unsupported commit type in subject: ${subject}"
  else
    append_entry "${other_file}" "" "" "${subject}"
  fi
  entry_count=$((entry_count + 1))
done < <(git log --no-merges --pretty=format:%s "${FROM_REF}..${TO_REF}")

[[ "${entry_count}" -gt 0 ]] || die "no release note entries found in ${FROM_REF}..${TO_REF}"

render_notes() {
  printf '# Talos %s\n\n' "${TAG}"
  emit_section "New Features" "${features_file}"
  emit_section "Bug Fixes" "${fixes_file}"
  emit_section "Performance" "${performance_file}"
  emit_section "Improvements" "${improvements_file}"
  emit_section "Documentation" "${documentation_file}"
  emit_section "Build and CI" "${build_ci_file}"
  emit_section "Tests" "${tests_file}"
  emit_section "Tooling" "${tooling_file}"
  emit_section "Maintenance" "${maintenance_file}"
  emit_section "Reverts" "${reverts_file}"
  emit_section "Other Changes" "${other_file}"
  printf '## Full Changelog\n\n'
  printf '[%s...%s](https://github.com/%s/compare/%s...%s)\n' \
    "${FROM_REF}" "${TAG}" "${SOURCE_REPOSITORY}" "${FROM_REF}" "${TAG}"
}

if [[ -n "${OUTPUT_FILE}" ]]; then
  render_notes > "${OUTPUT_FILE}"
else
  render_notes
fi
