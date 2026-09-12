#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
validator="${script_dir}/check-commit-message.sh"

valid_subjects=(
    "feat: add picker"
    "feat(tui): add picker"
    "fix!: change public behavior"
    "fix(api)!: change public behavior"
    "perf(storage): reduce query allocations"
    "refactor(agent): separate execution policy"
    "docs(release): document release branches"
    "test(e2e): verify native installation"
    "build(desktop): package macOS application"
    "ci(release): dispatch artifact publishing"
    "chore(release): prepare v0.1.8"
    "style(web): format settings form"
    "revert: restore previous behavior"
    "tool(release): update catalog"
)

invalid_subjects=(
    ""
    "tooling: update build scripts"
    "feature: add picker"
    "Release v0.1.8"
    "Prepare 0.1.8 release"
    "Merge pull request #10"
    "feat : invalid spacing"
    "fix(scope) missing colon"
)

for subject in "${valid_subjects[@]}"; do
    "${validator}" "${subject}"
done

for subject in "${invalid_subjects[@]}"; do
    if "${validator}" "${subject}" >/dev/null 2>&1; then
        echo "Expected commit subject to be rejected: ${subject}" >&2
        exit 1
    fi
done

temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT
git -C "${temp_dir}" init -q -b main
git -C "${temp_dir}" config user.name "Commit Test"
git -C "${temp_dir}" config user.email "commit-test@example.com"
printf 'base\n' > "${temp_dir}/changes"
git -C "${temp_dir}" add changes
git -C "${temp_dir}" commit -q -m "chore: initialize repository"
base="$(git -C "${temp_dir}" rev-parse HEAD)"
printf 'valid\n' >> "${temp_dir}/changes"
git -C "${temp_dir}" commit -qam "fix(test): validate commit range"
(cd "${temp_dir}" && "${validator}" --range "${base}" "$(git rev-parse HEAD)")
printf 'invalid\n' >> "${temp_dir}/changes"
git -C "${temp_dir}" commit -qam "Invalid range subject"
if (cd "${temp_dir}" && "${validator}" --range "${base}" "$(git rev-parse HEAD)") >/dev/null 2>&1; then
  echo "Expected invalid subject in revision range to be rejected" >&2
  exit 1
fi
