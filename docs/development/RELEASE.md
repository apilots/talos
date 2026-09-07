# Release workflow

[Chinese](RELEASE_zh.md)

The root `release.sh` script creates an annotated Git tag and GitHub Release.
Tag-triggered CI workflows build, package, and publish product artifacts.

## Preconditions

- The version change is merged to `main`.
- Rust, TUI, Web, Desktop, and TypeScript SDK versions match.
- Local `main` is clean and synchronized with `origin/main`.
- GitHub CLI is installed and authenticated.
- Release notes exist in a non-empty Markdown file.

## Run

```bash
./release.sh 0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md
./release.sh v0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md --yes --watch
./release.sh 0.1.7 --notes /tmp/talos-v0.1.7-release-notes.md --dry-run
```

The script validates repository state, version consistency, existing tags, and
existing GitHub Releases. It does not modify version files or upload locally
built artifacts.

## Verification

Use `--dry-run` to inspect the command path without creating a tag or Release.
After release creation, verify the tag-triggered workflow and all target-specific
assets. The tag-triggered dispatch waits for the complete native installation
matrix before starting the release build. Pull requests run only the inexpensive
installer syntax and contract checks when release-related files change. The full
matrix can also be started explicitly with `workflow_dispatch`. Installation
acceptance is owned by E2E group `15_tui_install`.

```text
Merged version + release notes
             |
             v
release.sh -> annotated tag + GitHub Release
             |
             v
tag workflow -> complete native installation matrix
             |
         success only
             v
dispatch release build -> verify published target artifacts
```

Protocol/data changes also require coordinated consumer rollout and a tested
backup/rollback procedure; see [Deployment](../../deploy/README.md#data-upgrades-and-rollback).
Install success alone does not establish API compatibility or safe data rollback.
