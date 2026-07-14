#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "${repo_root}"

python3 scripts/check_changed_files.py --cached

web_files=()
desktop_files=()
sdk_core_files=()
sdk_app_server_files=()
sdk_browser_files=()
shell_files=()
rust_files=()

while IFS= read -r -d '' path; do
    case "${path}" in
        talos-app/web/*.ts|talos-app/web/*.tsx)
            web_files+=("${path#talos-app/web/}")
            ;;
        talos-app/desktop/*.ts|talos-app/desktop/*.tsx)
            desktop_files+=("${path#talos-app/desktop/}")
            ;;
        talos-app/ts-sdk/packages/core/*.ts|talos-app/ts-sdk/packages/core/*.tsx)
            sdk_core_files+=("${path#talos-app/ts-sdk/packages/core/}")
            ;;
        talos-app/ts-sdk/packages/app-server/*.ts|talos-app/ts-sdk/packages/app-server/*.tsx)
            sdk_app_server_files+=("${path#talos-app/ts-sdk/packages/app-server/}")
            ;;
        talos-app/ts-sdk/packages/browser/*.ts|talos-app/ts-sdk/packages/browser/*.tsx)
            sdk_browser_files+=("${path#talos-app/ts-sdk/packages/browser/}")
            ;;
    esac
    [[ "${path}" == *.sh ]] && shell_files+=("${path}")
    [[ "${path}" == talos-rs/*.rs ]] && rust_files+=("${path}")
done < <(git diff --cached --name-only --diff-filter=AM --no-renames -z)

require_no_unstaged_changes() {
    local path="$1"
    if ! git diff --quiet -- "${path}"; then
        echo "Cannot validate staged content while the file also has unstaged changes: ${path}" >&2
        exit 1
    fi
}

run_eslint() {
    local package_dir="$1"
    shift
    [[ "$#" -gt 0 ]] || return 0
    command -v pnpm >/dev/null 2>&1 || {
        echo "pnpm is required to lint staged frontend files." >&2
        exit 1
    }
    for path in "$@"; do
        require_no_unstaged_changes "${package_dir}/${path}"
    done
    (cd "${package_dir}" && pnpm exec eslint "$@")
}

for path in "${shell_files[@]}"; do
    require_no_unstaged_changes "${path}"
    bash -n "${path}"
done

if [[ "${#web_files[@]}" -gt 0 ]]; then
    run_eslint talos-app/web "${web_files[@]}"
fi
if [[ "${#desktop_files[@]}" -gt 0 ]]; then
    run_eslint talos-app/desktop "${desktop_files[@]}"
fi
if [[ "${#sdk_core_files[@]}" -gt 0 ]]; then
    run_eslint talos-app/ts-sdk/packages/core "${sdk_core_files[@]}"
fi
if [[ "${#sdk_app_server_files[@]}" -gt 0 ]]; then
    run_eslint talos-app/ts-sdk/packages/app-server "${sdk_app_server_files[@]}"
fi
if [[ "${#sdk_browser_files[@]}" -gt 0 ]]; then
    run_eslint talos-app/ts-sdk/packages/browser "${sdk_browser_files[@]}"
fi

if [[ "${#rust_files[@]}" -gt 0 ]]; then
    for path in "${rust_files[@]}"; do
        require_no_unstaged_changes "${path}"
    done
    (cd talos-rs && cargo fmt --all -- --check)
fi
