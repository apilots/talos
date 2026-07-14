#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
validator="${script_dir}/check-commit-message.sh"

valid_subjects=(
    "feat: add picker"
    "feat(tui): add picker"
    "fix!: change public behavior"
    "fix(api)!: change public behavior"
    "tool(release): update catalog"
    "Update version 0.1.3 to 0.2.0"
    "Prepare 0.2.0 release"
    "Merge pull request #10"
)

invalid_subjects=(
    ""
    "tooling: update build scripts"
    "feature: add picker"
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
