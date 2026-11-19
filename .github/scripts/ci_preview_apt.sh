#!/usr/bin/env bash
set -euo pipefail

mode=${1:?expected download or install}
cache_dir=${2:?expected a cache directory}
case "$mode" in
  download) apt_mode=(--download-only) ;;
  install) apt_mode=() ;;
  *) echo "Unknown preview apt mode: $mode" >&2; exit 2 ;;
esac

mapfile -t lock < "$(dirname "$0")/preview-office.lock"
mkdir -p "$cache_dir"/{archives,lists}
source_file=$(mktemp --suffix=.sources)
trap 'rm -f "$source_file"' EXIT
cat > "$source_file" <<EOF
Types: deb
URIs: https://snapshot.ubuntu.com/ubuntu/${lock[0]}/
Suites: noble noble-updates noble-security
Components: main universe
Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg
Check-Valid-Until: no
EOF
chmod 644 "$source_file"

export DEBIAN_FRONTEND=noninteractive
apt=(apt-get)
if (( EUID != 0 )); then
  apt=(sudo --preserve-env=DEBIAN_FRONTEND "${apt[@]}")
fi
apt+=(
  -o "Dir::Etc::SourceList=$source_file"
  -o Dir::Etc::SourceParts=-
  -o "Dir::State::lists=$cache_dir/lists"
  -o "Dir::Cache::archives=$cache_dir/archives"
  -o Acquire::Languages=none
  -o Acquire::Retries=2
  -o Acquire::https::Timeout=30
)

"${apt[@]}" update --error-on=any
"${apt[@]}" install -y --no-install-recommends "${apt_mode[@]}" "${lock[@]:1}"
echo "Preview apt $mode completed in ${SECONDS}s"
