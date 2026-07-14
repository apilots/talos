#!/usr/bin/env bash
set -euo pipefail

subject="${1:-}"
allowed_types='feat|fix|docs|style|test|refactor|tool|revert|chore'
pattern="^(${allowed_types})(\([a-z0-9][a-z0-9._/-]*\))?(!)?: [^[:space:]].*$"

if [[ "${subject}" =~ ${pattern} ]]; then
  exit 0
fi

cat >&2 <<EOF
Invalid commit subject: ${subject}

Expected one of:
  <type>: <description>
  <type>(<scope>): <description>

Allowed types: ${allowed_types//|/, }
EOF
exit 1
