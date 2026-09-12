#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
generator="${repo_root}/scripts/generate-release-notes.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

git -C "${temp_dir}" init -q -b main
git -C "${temp_dir}" config user.name "Release Test"
git -C "${temp_dir}" config user.email "release-test@example.com"

commit_subject() {
  local subject="$1"
  printf '%s\n' "${subject}" >> "${temp_dir}/changes"
  git -C "${temp_dir}" add changes
  git -C "${temp_dir}" commit -q -m "${subject}"
}

commit_subject "chore: initialize repository"
git -C "${temp_dir}" tag v0.1.6

subjects=(
  "feat(api): add release endpoint"
  "fix: reject invalid version"
  "perf(store): reduce release lookup time"
  "refactor(agent): separate release policy"
  "docs: document releases"
  "test(e2e): verify release workflow"
  "build(desktop): package application"
  "ci(release): dispatch artifacts"
  "chore(ci): maintain legacy CI"
  "chore: update dependencies"
  "style(web): format release page"
  "revert: restore release behavior"
  "tool(release): add notes generator"
  "Legacy release note"
  "chore(release): prepare v0.1.7"
  "chore(release): start v0.1.8 development"
)
for subject in "${subjects[@]}"; do
  commit_subject "${subject}"
done

git -C "${temp_dir}" switch -q -c release-test-topic
commit_subject "feat(topic): include merged feature once"
git -C "${temp_dir}" switch -q main
git -C "${temp_dir}" merge -q --no-ff release-test-topic -m "Merge pull request #1"

notes="${temp_dir}/release-notes.md"
(
  cd "${temp_dir}"
  "${generator}" v0.1.7 --from v0.1.6 --output "${notes}"
)

expected_sections=(
  "## New Features"
  "## Bug Fixes"
  "## Performance"
  "## Improvements"
  "## Documentation"
  "## Build and CI"
  "## Tests"
  "## Tooling"
  "## Maintenance"
  "## Reverts"
  "## Other Changes"
  "## Full Changelog"
)
for section in "${expected_sections[@]}"; do
  grep -Fqx "${section}" "${notes}"
done

grep -Fqx -- "- **api:** add release endpoint" "${notes}"
grep -Fqx -- "- Legacy release note" "${notes}"
grep -Fqx -- "- **ci:** maintain legacy CI" "${notes}"
[[ "$(grep -Fc "include merged feature once" "${notes}")" == "1" ]]
! grep -Fq "Merge pull request" "${notes}"
! grep -Fq "prepare v0.1.7" "${notes}"
! grep -Fq "start v0.1.8 development" "${notes}"
grep -Fqx '[v0.1.6...v0.1.7](https://github.com/apilots/talos/compare/v0.1.6...v0.1.7)' "${notes}"
