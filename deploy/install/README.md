# Talos TUI installer

[Chinese](README_zh.md)

The installer selects the TUI release bundle for the current macOS or Linux CPU target,
verifies its release checksum, installs immutable product resources, and activates a stable
launcher.

Install the latest release:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos/releases/latest/download/install.sh | sh
```

Install a specific version:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos/releases/latest/download/install.sh \
  | sh -s -- --version 0.1.7
```

Default paths:

```text
~/.local/share/talos/versions/<version>-<platform>-<arch>
~/.local/share/talos/current
~/.local/bin/talos
```

Override installation paths with absolute `TALOS_INSTALL_ROOT` and `TALOS_BIN_DIR` values.
Reinstalling the same verified version is idempotent and reports that it is already installed.
The installer does not write `$TALOS_HOME` or `~/.talos`; Talos startup continues to own
config upgrades, runtime migrations, databases, logs, and workspaces.

Run the offline installer tests with:

```bash
deploy/install/tests/install_test.sh
```

`release_assets_test.sh` additionally verifies and installs from the fully assembled four-platform
release asset directory. The release workflow runs it after generating the final checksum manifest.
The installer checks Talos releases first and falls back to archived `talos-releases` assets
for versions that predate the migration.

Run native and available Docker acceptance paths with:

```bash
just e2e-group 15_tui_install
```

Docker validates Linux only. macOS bundles must run on a native macOS host or GitHub-hosted
macOS runner.
