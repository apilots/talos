#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "${repo_root}"

zero_sha="0000000000000000000000000000000000000000"
frontend_changed=false
rust_changed=false
checked_ref=false

while read -r local_ref local_sha remote_ref remote_sha; do
    [[ -n "${local_ref}" && "${local_sha}" != "${zero_sha}" ]] || continue
    checked_ref=true
    base="${remote_sha}"
    if [[ "${base}" == "${zero_sha}" ]]; then
        base="$(git merge-base "${local_sha}" origin/main 2>/dev/null || true)"
        if [[ -z "${base}" ]]; then
            base="${local_sha}^"
        fi
    fi

    python3 scripts/check_changed_files.py --base "${base}" --head "${local_sha}"
    scripts/check-commit-message.sh --range "${base}" "${local_sha}"
    while IFS= read -r -d '' path; do
        [[ "${path}" == talos-app/* ]] && frontend_changed=true
        [[ "${path}" == talos-rs/* || "${path}" == justfile ]] && rust_changed=true
        if [[ "${path}" == *.sh ]] && git cat-file -e "${local_sha}:${path}" 2>/dev/null; then
            git show "${local_sha}:${path}" | bash -n
        fi
    done < <(git diff "${base}" "${local_sha}" --name-only --diff-filter=AM --no-renames -z)
done

if [[ "${checked_ref}" != true ]]; then
    echo "No pushed commits require validation."
    exit 0
fi

if [[ "${frontend_changed}" == true ]]; then
    command -v pnpm >/dev/null 2>&1 || {
        echo "pnpm is required for frontend pre-push checks." >&2
        exit 1
    }
    (cd talos-app && pnpm ci:frontend-app)
fi

if [[ "${rust_changed}" == true ]]; then
    command -v just >/dev/null 2>&1 || {
        echo "just is required for Rust pre-push checks." >&2
        exit 1
    }
    just fmt-check
    python3 scripts/check_error_contract_tests.py
    python3 scripts/check_error_contract.py
    just check
fi
