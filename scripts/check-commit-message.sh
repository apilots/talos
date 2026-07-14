#!/usr/bin/env bash
set -euo pipefail

subject="${1:-}"
allowed_types='feat|fix|docs|style|test|refactor|tool|revert|chore'
valid_pattern="^(${allowed_types})(\([a-z0-9][a-z0-9._/-]*\))?(!)?: [^[:space:]].*$"
typed_pattern='^[a-z][a-z0-9-]*(\([^)]*\))?(!)?:'
reserved_type_pattern="^(${allowed_types})([[:space:]]|\(|!|:)"

if [[ "${subject}" =~ ${valid_pattern} ]]; then
    exit 0
fi

if [[ -n "${subject//[[:space:]]/}" ]] \
    && [[ ! "${subject}" =~ ${typed_pattern} ]] \
    && [[ ! "${subject}" =~ ${reserved_type_pattern} ]]; then
    exit 0
fi

cat >&2 <<EOF
Invalid commit subject: ${subject}

Supported categorized subjects:
  <type>: <description>
  <type>(<scope>): <description>

Allowed types: ${allowed_types//|/, }

Plain subjects without a type prefix are allowed and appear under Other in release notes.
EOF
exit 1
