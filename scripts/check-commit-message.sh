#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "${script_dir}/commit-types.sh"

valid_pattern="^(${COMMIT_TYPES_PATTERN})(\([a-z0-9][a-z0-9._/-]*\))?(!)?: [^[:space:]].*$"

validate_subject() {
  local subject="$1"
  if [[ "${subject}" =~ ${valid_pattern} ]]; then
    return
  fi

  cat >&2 <<EOF
Invalid commit subject: ${subject}

Expected one of:
  <type>: <description>
  <type>(<scope>): <description>

Allowed types: ${COMMIT_TYPES_PATTERN//|/, }

EOF
  return 1
}

if [[ "${1:-}" == "--range" ]]; then
  [[ $# -eq 3 ]] || {
    echo "Usage: $0 --range <base> <head>" >&2
    exit 2
  }
  git rev-parse --verify "$2^{commit}" >/dev/null
  git rev-parse --verify "$3^{commit}" >/dev/null
  while IFS= read -r -d '' subject; do
    validate_subject "${subject}"
  done < <(git log --no-merges -z --format='%s' "$2..$3")
  exit 0
fi

[[ $# -eq 1 ]] || {
  echo "Usage: $0 <subject>" >&2
  exit 2
}
validate_subject "$1"
