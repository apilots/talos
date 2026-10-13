#!/bin/sh
set -eu

PROGRAM_NAME="talos-install"
DEFAULT_RELEASE_BASE_URL="https://github.com/apilots/talos/releases"
LEGACY_RELEASE_BASE_URL="https://github.com/apilots/talos-releases/releases"
release_url_override="${TALOS_RELEASE_BASE_URL:-}"
release_base_url="${release_url_override:-${DEFAULT_RELEASE_BASE_URL}}"
requested_version="${TALOS_VERSION:-}"
temp_dir=""
staging_dir=""
current_temp=""
lock_dir=""
lock_acquired=0
atomic_mv_mode=""

usage() {
  cat <<'EOF'
Install the Talos TUI release bundle for the current platform.

Usage:
  install.sh [--version <version>]

Environment:
  TALOS_VERSION             Release version when --version is omitted.
  TALOS_INSTALL_ROOT        Managed installation root.
                            Default: $XDG_DATA_HOME/talos or $HOME/.local/share/talos
  TALOS_BIN_DIR             User command directory. Default: $HOME/.local/bin
  TALOS_RELEASE_BASE_URL    Release URL override for mirrors and testing.
EOF
}

die() {
  printf '%s: %s\n' "${PROGRAM_NAME}" "$*" >&2
  exit 1
}

cleanup() {
  if [ -n "${staging_dir}" ] && [ -d "${staging_dir}" ]; then
    rm -rf "${staging_dir}" || true
  fi
  if [ -n "${temp_dir}" ] && [ -d "${temp_dir}" ]; then
    rm -rf "${temp_dir}" || true
  fi
  if [ -n "${current_temp}" ] && [ -L "${current_temp}" ]; then
    rm -f "${current_temp}" || true
  fi
  if [ "${lock_acquired}" -eq 1 ] && [ -n "${lock_dir}" ]; then
    rmdir "${lock_dir}" 2>/dev/null || true
  fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

require_command() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

download_file() {
  url="$1"
  output="$2"
  case "${url}" in
    https://*) curl --proto '=https' --tlsv1.2 -fsSL "${url}" -o "${output}" ;;
    file://*) curl -fsSL "${url}" -o "${output}" ;;
    *) die "release URL must use https or file: ${url}" ;;
  esac
}

resolve_latest_version() {
  latest_url="${release_base_url}/latest"
  case "${latest_url}" in
    https://*)
      effective_url="$(curl --proto '=https' --tlsv1.2 -fsSL \
        -o /dev/null -w '%{url_effective}' "${latest_url}")" || return 1
      ;;
    *) return 1 ;;
  esac
  latest_tag="${effective_url%/}"
  latest_tag="${latest_tag##*/}"
  case "${latest_tag}" in
    v?*) printf '%s\n' "${latest_tag#v}" ;;
    *) return 1 ;;
  esac
}

try_download_file() {
  url="$1"
  output="$2"
  case "${url}" in
    https://*) curl --proto '=https' --tlsv1.2 -fsSL "${url}" -o "${output}" ;;
    file://*) curl -fsSL "${url}" -o "${output}" ;;
    *) return 1 ;;
  esac
}

sha256_file() {
  path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "${path}" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "${path}" | awk '{print $1}'
  else
    die "sha256sum or shasum is required"
  fi
}

validate_bundle() {
  bundle_root="$1"
  bundle_symlink="$(find "${bundle_root}" -type l -print -quit)"
  [ -z "${bundle_symlink}" ] || die "bundle contains an unsupported symlink: ${bundle_symlink}"
  [ -x "${bundle_root}/bin/talos" ] || die "bundle is missing executable bin/talos"
  [ -f "${bundle_root}/conf/talos.yaml" ] || die "bundle is missing conf/talos.yaml"
  [ -f "${bundle_root}/conf/migrations/postgres/README.md" ] \
    || die "bundle is missing the PostgreSQL migration marker"
  set -- "${bundle_root}"/conf/migrations/sqlite/*.sql
  [ -f "$1" ] || die "bundle is missing SQLite migrations"
}

write_bundle_manifest() {
  bundle_root="$1"
  manifest_path="$2"
  (
    cd "${bundle_root}"
    find bin conf -type f -print | LC_ALL=C sort | while IFS= read -r relative_path; do
      file_checksum="$(sha256_file "${relative_path}")"
      printf '%s  %s\n' "${file_checksum}" "${relative_path}"
    done
  ) >"${manifest_path}"
}

validate_installed_bundle() {
  bundle_root="$1"
  checksum_marker="${bundle_root}/.talos-release-checksum"
  installed_manifest="${bundle_root}/.talos-bundle-manifest"
  generated_manifest="${temp_dir}/installed-bundle-manifest"

  validate_bundle "${bundle_root}"
  [ -f "${checksum_marker}" ] || die "installed bundle is missing its checksum marker: ${bundle_root}"
  installed_checksum="$(cat "${checksum_marker}")"
  [ "${#installed_checksum}" -eq 64 ] \
    || die "installed bundle has an invalid checksum marker: ${bundle_root}"
  case "${installed_checksum}" in
    *[!0-9A-Fa-f]*) die "installed bundle has an invalid checksum marker: ${bundle_root}" ;;
  esac
  [ -f "${installed_manifest}" ] || die "installed bundle is missing its content manifest: ${bundle_root}"
  write_bundle_manifest "${bundle_root}" "${generated_manifest}"
  cmp -s "${installed_manifest}" "${generated_manifest}" \
    || die "installed bundle content has changed: ${bundle_root}"
}

validate_archive_members() {
  archive="$1"
  member_list="${temp_dir}/archive-members.txt"
  tar -tzf "${archive}" >"${member_list}" || die "could not list release archive"
  while IFS= read -r member; do
    case "${member}" in
      /*|../*|*/../*|*/..) die "release archive contains an unsafe path: ${member}" ;;
    esac
  done <"${member_list}"
}

write_launcher() {
  launcher_dir="$1"
  launcher_path="${launcher_dir}/talos"
  launcher_temp="${launcher_dir}/.talos-launcher.$$"
  mkdir -p "${launcher_dir}"
  cat >"${launcher_temp}" <<'EOF'
#!/bin/sh
set -eu

launcher_path="$0"
while [ -L "${launcher_path}" ]; do
  link_target="$(readlink "${launcher_path}")"
  case "${link_target}" in
    /*) launcher_path="${link_target}" ;;
    *) launcher_path="$(dirname "${launcher_path}")/${link_target}" ;;
  esac
done
launcher_dir="$(CDPATH= cd -- "$(dirname -- "${launcher_path}")" && pwd)"
install_root="$(dirname "${launcher_dir}")"
if ! resource_dir="$(CDPATH= cd -- "${install_root}/current" && pwd -P)"; then
  printf 'talos: active installation is missing under %s\n' "${install_root}" >&2
  exit 1
fi
export TALOS_RESOURCE_DIR="${TALOS_RESOURCE_DIR:-${resource_dir}}"
exec "${resource_dir}/bin/talos" "$@"
EOF
  chmod 755 "${launcher_temp}"
  mv -f "${launcher_temp}" "${launcher_path}"
}

replace_symlink_atomically() {
  source_path="$1"
  destination_path="$2"
  case "${atomic_mv_mode}" in
    no-target-directory) mv -Tf "${source_path}" "${destination_path}" ;;
    no-follow) mv -hf "${source_path}" "${destination_path}" ;;
    *) die "atomic move mode was not initialized" ;;
  esac
}

acquire_install_lock() {
  mkdir -p "${install_root}"
  lock_dir="${install_root}/.install-lock"
  if ! mkdir "${lock_dir}" 2>/dev/null; then
    die "another installation is active; remove stale lock only after verifying no installer is running: ${lock_dir}"
  fi
  lock_acquired=1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ "$#" -ge 2 ] || die "--version requires a value"
      requested_version="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *) die "unknown argument: $1" ;;
  esac
done

for command_name in curl tar mktemp awk tr find sort cmp cat grep mkdir rmdir rm mv ln chmod readlink dirname uname; do
  require_command "${command_name}"
done

[ -n "${HOME:-}" ] || die "HOME must be set"
release_base_url="${release_base_url%/}"

platform=""
os_name="$(uname -s)"
case "${os_name}" in
  Linux) platform="linux" ;;
  Darwin) platform="macos" ;;
  *) die "unsupported operating system: ${os_name}" ;;
esac

arch=""
machine_arch="$(uname -m)"
case "${machine_arch}" in
  x86_64|amd64) arch="amd64" ;;
  arm64|aarch64) arch="arm64" ;;
  *) die "unsupported architecture: ${machine_arch}" ;;
esac

if mv --help 2>&1 | grep -q -- '-T'; then
  atomic_mv_mode="no-target-directory"
else
  atomic_mv_mode="no-follow"
fi

if [ -z "${requested_version}" ]; then
  if [ -n "${release_url_override}" ]; then
    requested_version="$(resolve_latest_version)" \
      || die "could not resolve the latest version from ${release_base_url}"
  elif ! requested_version="$(resolve_latest_version)"; then
    printf 'Talos releases are not available yet; checking archived releases.\n' >&2
    release_base_url="${LEGACY_RELEASE_BASE_URL}"
    requested_version="$(resolve_latest_version)" \
      || die "could not resolve the latest version from ${release_base_url}"
  fi
fi
version="${requested_version#v}"
[ -n "${version}" ] || die "release version must not be empty"
case "${version}" in
  *[!A-Za-z0-9._-]*) die "invalid release version: ${requested_version}" ;;
esac

install_root="${TALOS_INSTALL_ROOT:-${XDG_DATA_HOME:-${HOME}/.local/share}/talos}"
bin_dir="${TALOS_BIN_DIR:-${HOME}/.local/bin}"
case "${install_root}" in
  /*) ;;
  *) die "TALOS_INSTALL_ROOT must be an absolute path: ${install_root}" ;;
esac
case "${bin_dir}" in
  /*) ;;
  *) die "TALOS_BIN_DIR must be an absolute path: ${bin_dir}" ;;
esac
launcher_dir="${install_root}/launcher"
launcher_target="${launcher_dir}/talos"
command_path="${bin_dir}/talos"
version_name="${version}-${platform}-${arch}"
version_dir="${install_root}/versions/${version_name}"
asset_name="Talos-TUI-${version}-${platform}-${arch}.tar.gz"
  download_base="${release_base_url}/download/v${version}"

acquire_install_lock
if [ -e "${command_path}" ] || [ -L "${command_path}" ]; then
  if [ ! -L "${command_path}" ] || [ "$(readlink "${command_path}")" != "${launcher_target}" ]; then
    die "refusing to overwrite unrelated command: ${command_path}"
  fi
fi
if [ -e "${install_root}/current" ] && [ ! -L "${install_root}/current" ]; then
  die "refusing to replace non-symlink current path: ${install_root}/current"
fi

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/talos-install.XXXXXX")"
mkdir -p "${install_root}/versions"
already_installed=0
if [ -d "${version_dir}" ]; then
  validate_installed_bundle "${version_dir}"
  already_installed=1
else
  checksums_path="${temp_dir}/checksums.txt"
  archive_path="${temp_dir}/${asset_name}"

  printf 'Downloading Talos TUI %s for %s-%s...\n' "${version}" "${platform}" "${arch}"
  if [ -n "${release_url_override}" ]; then
    download_file "${download_base}/checksums.txt" "${checksums_path}"
    download_file "${download_base}/${asset_name}" "${archive_path}"
  elif ! try_download_file "${download_base}/checksums.txt" "${checksums_path}" \
    || ! try_download_file "${download_base}/${asset_name}" "${archive_path}"; then
    rm -f "${checksums_path}" "${archive_path}"
    printf 'Talos release v%s is not available; checking the archived release.\n' "${version}" >&2
    release_base_url="${LEGACY_RELEASE_BASE_URL}"
    download_base="${release_base_url}/download/v${version}"
    download_file "${download_base}/checksums.txt" "${checksums_path}"
    download_file "${download_base}/${asset_name}" "${archive_path}"
  fi

  expected_checksum="$(awk -v target="${asset_name}" '
    {
      path = $2
      sub(/^\.\//, "", path)
      if (path == target) {
        print tolower($1)
        exit
      }
    }
  ' "${checksums_path}")"
  [ -n "${expected_checksum}" ] || die "checksum entry not found for ${asset_name}"
  actual_checksum="$(sha256_file "${archive_path}" | tr '[:upper:]' '[:lower:]')"
  [ "${actual_checksum}" = "${expected_checksum}" ] \
    || die "checksum mismatch for ${asset_name}"

  validate_archive_members "${archive_path}"
  staging_dir="${install_root}/versions/.${version_name}.installing.$$"
  mkdir "${staging_dir}"
  tar -xzf "${archive_path}" -C "${staging_dir}"
  validate_bundle "${staging_dir}"
  printf '%s\n' "${actual_checksum}" >"${staging_dir}/.talos-release-checksum"
  write_bundle_manifest "${staging_dir}" "${staging_dir}/.talos-bundle-manifest"
  mv "${staging_dir}" "${version_dir}"
  staging_dir=""
fi

write_launcher "${launcher_dir}"
current_temp="${install_root}/.current.$$"
ln -s "versions/${version_name}" "${current_temp}"
replace_symlink_atomically "${current_temp}" "${install_root}/current" \
  || die "could not activate Talos TUI ${version}"
current_temp=""

mkdir -p "${bin_dir}"
if [ ! -L "${command_path}" ]; then
  ln -s "${launcher_target}" "${command_path}"
fi

if [ "${already_installed}" -eq 1 ]; then
  printf 'Talos TUI %s is already installed at %s\n' "${version}" "${version_dir}"
else
  printf 'Talos TUI %s installed at %s\n' "${version}" "${version_dir}"
fi
printf 'Command: %s\n' "${command_path}"
case ":${PATH}:" in
  *":${bin_dir}:"*) ;;
  *) printf 'Add %s to PATH to run talos directly.\n' "${bin_dir}" ;;
esac
