# Talos TUI Release Bundle Installer

## Problem

Talos TUI is published as a target-specific release bundle containing the native `talos`
binary, the current default configuration template, and database migrations. The binary
requires `TALOS_RESOURCE_DIR` to point at that bundle. Copying only `bin/talos` into `PATH`
therefore does not produce a valid installation.

The release archive already distinguishes operating system and CPU architecture, but Talos
does not provide a public installer, macOS TUI release artifacts, or a product-level E2E group
that proves an installed bundle can start with its packaged resources.

## Goals

- Provide one public POSIX shell installer for macOS and Linux.
- Keep one canonical bundle layout across all targets.
- Support `amd64` and `arm64` target selection.
- Install immutable release resources separately from `$TALOS_HOME` runtime data.
- Verify release checksums before changing the active installation.
- Make installation and version switching idempotent and atomic.
- Add one independently runnable E2E group for native and Linux-container installation paths.
- Publish and test TUI artifacts on native GitHub-hosted runners for every supported target.

## Non-goals

- npm distribution.
- Windows installation.
- Static Linux linking or broad distribution compatibility work.
- Installer-owned migration of `$TALOS_HOME` configuration or database state.
- macOS validation inside Docker. Docker containers provide a Linux userland and cannot run
  Mach-O executables.

## Release bundle contract

Every target uses the same internal archive layout:

```text
bin/talos
conf/talos.yaml
conf/migrations/sqlite/*.sql
conf/migrations/postgres/README.md
```

Target-specific archive names are:

```text
Talos-TUI-<version>-linux-amd64.tar.gz
Talos-TUI-<version>-linux-arm64.tar.gz
Talos-TUI-<version>-macos-amd64.tar.gz
Talos-TUI-<version>-macos-arm64.tar.gz
```

The release also publishes `checksums.txt` and `install.sh`. The installer never infers a
different bundle layout for one target.

## Installation layout

The default user installation is:

```text
~/.local/share/talos/
  launcher/talos
  current -> versions/<version>-<platform>-<arch>
  versions/<version>-<platform>-<arch>/
    bin/talos
    conf/...
~/.local/bin/talos -> ~/.local/share/talos/launcher/talos
```

`TALOS_INSTALL_ROOT` and `TALOS_BIN_DIR` may override the two roots. The stable launcher
resolves its own installation root, sets `TALOS_RESOURCE_DIR` only when the caller has not
provided an override, and executes `current/bin/talos`.

The installer does not set `TALOS_HOME` and never writes to `~/.talos`. First-run config,
template upgrades, and database migration resources remain owned by the existing Talos
startup lifecycle.

## Installer flow

1. Validate required commands and arguments.
2. Map `uname -s` and `uname -m` to the release target.
3. Resolve the requested version, defaulting to the latest GitHub Release.
4. Download `checksums.txt` and the exact target archive into a temporary directory.
5. Select the exact checksum entry and verify the archive.
6. Reject unsafe archive member paths and validate the extracted bundle contract.
7. Atomically rename the staged version directory into `versions/`.
8. Atomically switch `current` to the selected version.
9. Install the stable launcher and create the user-facing symlink without overwriting an
   unrelated existing command.

An existing complete version directory is reused. A checksum or extraction failure occurs
before `current` changes.

## E2E strategy

The `15_tui_install` group owns installation acceptance.

- `01_native_install.sh` builds and installs the current host target, then executes the
  installed binary and embedded runtime.
- `02_linux_docker.sh` runs only on a macOS host and repeats the group-level Linux path in an
  Ubuntu 24.04 container for the Docker daemon architecture.
- CI runs `01_native_install.sh` on native Linux AMD64, Linux ARM64, macOS AMD64, and macOS
  ARM64 runners. This is the authoritative multi-platform matrix.

The E2E fixture uses a local `file://` release tree with the production archive naming and
checksum format. It does not depend on GitHub or R2 availability.

## Error handling and safety

- Unsupported operating systems and architectures fail explicitly.
- Missing checksum entries, checksum mismatches, unsafe archive paths, and incomplete bundle
  contents fail before activation.
- The installer does not silently replace an unrelated file at the requested bin path.
- Temporary data is cleaned on exit.
- The launcher preserves explicit `TALOS_RESOURCE_DIR` overrides.

## Compatibility impact

This adds release assets and supported installation entrypoints without changing the TUI
wire protocol, runtime config schema, SDK types, or `$TALOS_HOME` semantics. Existing release
archive paths remain unchanged.
