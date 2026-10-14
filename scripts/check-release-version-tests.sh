#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
validator="${repo_root}/scripts/check-release-version.sh"
temp_dir="$(mktemp -d)"
trap 'rm -rf "${temp_dir}"' EXIT

git -C "${temp_dir}" init -q -b release-1.2.3
git -C "${temp_dir}" config user.name "Release Test"
git -C "${temp_dir}" config user.email "release-test@example.com"
mkdir -p "${temp_dir}/scripts"
cp "${validator}" "${temp_dir}/scripts/check-release-version.sh"
git -C "${temp_dir}" config core.hooksPath /dev/null
git -C "${temp_dir}" add scripts/check-release-version.sh
git -C "${temp_dir}" -c core.hooksPath=/dev/null commit -q --allow-empty -m "chore: initialize"
cd "${temp_dir}"
python3 - <<'PY'
from pathlib import Path

files = {
    "talos-rs/Cargo.toml": '[workspace.package]\nversion = "1.2.3"\n',
    "talos-rs/apps/tui/src/utils.rs": 'pub const APP_VERSION: &str = "v1.2.3";\n',
    "talos-app/ts-sdk/packages/app-server/src/stdio.ts": 'version: this.options.clientVersion ?? "1.2.3"\n',
    "deploy/docker/.env.example": 'TALOS_SERVER_VERSION=v1.2.3\n',
    "deploy/docker/docker-compose.yml": 'image: ghcr.io/apilots/talos-server:${TALOS_SERVER_VERSION:-v1.2.3}\n',
}
for package in ("", "web/", "desktop/", "ts-sdk/", "ts-sdk/packages/core/",
                "ts-sdk/packages/browser/", "ts-sdk/packages/app-server/"):
    files[f"talos-app/{package}package.json"] = '{"version":"1.2.3"}\n'
for name, text in files.items():
    path = Path(name)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
PY

git add .
git -c core.hooksPath=/dev/null commit -q -m "chore(release): prepare v1.2.3"
bash "${validator}" 1.2.3
printf 'repair\n' > bugfix
git add bugfix
git -c core.hooksPath=/dev/null commit -q -m "fix(release): repair chore(release): prepare v1.2.3 validation"
bash "${validator}" 1.2.3

# Exercise the real publisher with a local bare origin and a GH CLI fixture.
# Git's default cleanup strips Markdown headings from annotated tag messages.
publication_root="${temp_dir}/.git/publication-test"
mkdir -p "${publication_root}/bin"
git init -q --bare "${publication_root}/origin.git"
git remote add origin "${publication_root}/origin.git"
git push -q -u origin release-1.2.3
cat > "${publication_root}/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  'auth status') exit 0 ;;
  'release view') exit 1 ;;
  'run list') printf '[]\n' ;;
  *) echo "Unexpected GH fixture request: $*" >&2; exit 64 ;;
esac
EOF
chmod +x "${publication_root}/bin/gh"
cat > "${publication_root}/notes.md" <<'EOF'
# Talos v1.2.3

## Changes

- Preserve Markdown headings and # inline text exactly.
EOF
PATH="${publication_root}/bin:${PATH}" bash "${repo_root}/release.sh" 1.2.3 \
  --notes "${publication_root}/notes.md" --yes > "${publication_root}/publisher.log" 2>&1
python3 - "${publication_root}" <<'PY_TAG'
import subprocess
import sys
from pathlib import Path

root = Path(sys.argv[1])
expected = (root / "notes.md").read_bytes()
for git_args in ([], [f"--git-dir={root / 'origin.git'}"]):
    tag = subprocess.check_output(["git", *git_args, "cat-file", "tag", "v1.2.3"])
    assert tag.split(b"\n\n", 1)[1] == expected, "Canonical Markdown notes changed in the tag"
    target = subprocess.check_output(["git", *git_args, "rev-parse", "v1.2.3^{}"])
    assert target == subprocess.check_output(["git", "rev-parse", "HEAD"])
PY_TAG

# Reject an invalid existing annotation before a push or remote-tag reuse.
remote_tag="$(git --git-dir="${publication_root}/origin.git" rev-parse v1.2.3)"
git tag -a --cleanup=verbatim -f v1.2.3 -m 'Invalid notes header' HEAD >/dev/null
if PATH="${publication_root}/bin:${PATH}" bash "${repo_root}/release.sh" 1.2.3 \
  --notes "${publication_root}/notes.md" --yes > "${publication_root}/rejected.log" 2>&1; then
  echo 'Invalid annotation should fail before remote publication' >&2
  exit 1
fi
grep -Fq 'tag annotation must start with: # Talos v1.2.3' "${publication_root}/rejected.log"
[[ "$(git --git-dir="${publication_root}/origin.git" rev-parse v1.2.3)" == "${remote_tag}" ]]

expect_rejection() {
  local reason="$1"
  if bash "${validator}" 1.2.3 >validation.log 2>&1; then
    echo "Expected rejection: ${reason}" >&2
    exit 1
  fi
  grep -Fq "${reason}" validation.log
}

git -c core.hooksPath=/dev/null commit -q --allow-empty -m "feat: add feature"
expect_rejection "release stabilization only accepts fix commits"
git reset --hard -q HEAD^
for subject in 'fix(/): invalid scope' 'fix: '; do
  git -c core.hooksPath=/dev/null commit -q --allow-empty -m "${subject}"
  expect_rejection "release stabilization only accepts fix commits"
  git reset --hard -q HEAD^
done
printf '{"version":"1.2.4-SNAPSHOT"}\n' > talos-app/package.json
expect_rejection "expected '1.2.3'"
git restore talos-app/package.json
git reset --soft HEAD~2
git -c core.hooksPath=/dev/null commit -q -m "chore(release): prepare v1.2.3"
expect_rejection "version preparation commit must contain only"

echo "Release version lifecycle tests passed."
