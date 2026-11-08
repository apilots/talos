#!/usr/bin/env bash
set -euo pipefail

directory=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
machine=${1:-$(uname -m)}
release=${2:-$(uname -r)}
case "$machine" in
  x86_64|amd64) arch=amd64; oci=SCMP_ARCH_X86_64 ;;
  aarch64|arm64) arch=arm64; oci=SCMP_ARCH_AARCH64 ;;
  *) echo "Unsupported Chromium architecture $machine" >&2; exit 1 ;;
esac

jq -e --arg arch "$arch" --arg oci "$oci" --arg release "$release" '
  def version: capture("^(?<major>[0-9]+)\\.(?<minor>[0-9]+)") | [.major, .minor] | map(tonumber);
  ($release | version) as $kernel |
  (.archMap[] | select(.architecture == $oci) | [.architecture] + .subArchitectures) as $architectures |
  if any(.syscalls[];
    ((.includes // {} | keys) - ["arches", "caps", "minKernel"] | length) > 0 or
    ((.excludes // {} | keys) - ["arches", "caps"] | length) > 0)
  then error("Unsupported conditional field in the Chromium seccomp profile")
  else
    {defaultAction, defaultErrnoRet, architectures: $architectures, syscalls: [
      .syscalls[] |
      # Chromium chroots its zygote after creating a private user namespace.
      # The kernel still checks CAP_SYS_CHROOT in that namespace.
      select((.includes.caps // [] | length) == 0 or
        (.action == "SCMP_ACT_ALLOW" and .names == ["chroot"] and .includes.caps == ["CAP_SYS_CHROOT"])) |
      select((.includes.arches // []) | length == 0 or index($arch) != null) |
      select((.excludes.arches // []) | index($arch) == null) |
      select((.includes.minKernel // "0.0" | version) <= $kernel) |
      del(.includes, .excludes)
    ]}
  end
' "$directory/../docker/talos-worker/chromium-seccomp.json"
