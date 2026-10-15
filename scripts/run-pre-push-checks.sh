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
    if [[ "${remote_ref}" == refs/tags/* ]]; then
        source_sha="$(git rev-parse "${local_sha}^{commit}")"
        tag="${remote_ref#refs/tags/}"
        if [[ "${tag}" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
            [[ "$(git cat-file -t "${local_sha}")" == tag ]] || {
                echo "Release tags must be annotated." >&2; exit 1;
            }
            title="$(git cat-file tag "${local_sha}" | sed '1,/^$/d' | sed -n '1p')"
            [[ "${title}" == "# Talos ${tag}" ]] || {
                echo "Release tag Notes must start with # Talos ${tag}." >&2; exit 1;
            }
            version="$(git show "${source_sha}:talos-rs/Cargo.toml" | awk '
                /^\[workspace.package\]/ { active=1; next } /^\[/ { active=0 }
                active && /^version = / { gsub(/"/, "", $3); print $3; exit }')"
            [[ "v${version}" == "${tag}" ]] || {
                echo "Release tag and source version differ." >&2; exit 1;
            }
        fi
        echo "Tag ${tag}: metadata checked; publisher and release CI require exact-source candidate evidence."
        continue
    fi
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
    for scope in frontend rust; do
        result="$(python3 .github/scripts/pr_change_scope.py "${scope}" \
            --base "${base}" --head "${local_sha}" --product-only --print-result)"
        if [[ "${result}" == true ]]; then
            if [[ "${scope}" == frontend ]]; then frontend_changed=true; else rust_changed=true; fi
        fi
    done
    while IFS= read -r -d '' path; do
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
