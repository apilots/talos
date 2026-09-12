# Release workflow

[Chinese](RELEASE_zh.md)

Talos publishes each version in two repositories. The source repository owns the
tag and canonical Release Notes. `apilots/talos-releases` reuses that body without
regenerating it and attaches the published product artifacts.

## Commit subjects

Non-merge commits use this form:

```text
<type>: <description>
<type>(<scope>): <description>
<type>!: <description>
<type>(<scope>)!: <description>
```

Allowed types are `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`,
`ci`, `chore`, `style`, `revert`, and `tool`. Git merge commits are exempt because
the commit hook detects them directly. Release version commits use exactly:

```text
chore(release): prepare v<version>
```

## Prepare a release

Record the remote `main` commit, then create the release branch from that exact
commit:

```bash
git fetch origin main --tags
release_base="$(git rev-parse origin/main)"
git switch -c release-0.1.8 "${release_base}"
test "$(git rev-parse HEAD)" = "${release_base}"
git push -u origin release-0.1.8
```

Generate an English Release Notes draft from the preceding release tag. Review
and edit the generated Markdown before publishing it.

```bash
scripts/generate-release-notes.sh v0.1.8 \
  --from v0.1.7 \
  --output /tmp/talos-v0.1.8-release-notes.md
```

The generator uses the commit subject type as the category, excludes merge and
release preparation commits, retains legacy untyped subjects under Other
Changes, and rejects unknown typed subjects. Empty categories are omitted.

Update all release version locations in one standalone commit:

- Rust workspace package version.
- TUI display version.
- Web, Desktop, and TypeScript SDK package versions, including the App Server
  SDK's default client metadata version.
- Docker example and Compose default image versions.

`talos-rs/Cargo.lock` is intentionally ignored and generated from the workspace
manifest; it is not part of the version commit. The release script requires the
exact commit subject above and rejects any file outside the listed version
locations.

The release branch must be clean, pushed, and synchronized with its matching
remote branch before `release.sh` runs.

## Validate

Run the checks appropriate to every change since the previous release, followed
by the release-wide gates:

```bash
just test-commit-messages
just test-release-notes
just fmt-check
just check
pnpm --dir talos-app ci:frontend-app
deploy/install/tests/install_test.sh
deploy/install/tests/release_contract_test.sh
```

Run relevant E2E scenarios for changed behavior. The release exception described
below requires `just test`, the frontend pipeline, installer contract tests, and
the affected E2E scenarios on the release host. Store their command output under
the ignored `output/release-<version>/` directory. Record unavailable platforms
or external-account validation as unverified instead of presenting it as passed.

## Publish the source release

Use `--dry-run` first. The script validates the branch, remote synchronization,
version commit, version consistency, source Release absence, and Notes heading.
It then pushes an annotated source tag and creates the canonical source Release.
If tag creation succeeded but Release creation failed, rerunning the same command
is safe only when the existing tag still resolves to the current `HEAD`; the
script checks that condition before reusing it.

```bash
./release.sh 0.1.8 --notes /tmp/talos-v0.1.8-release-notes.md --dry-run
./release.sh 0.1.8 --notes /tmp/talos-v0.1.8-release-notes.md --yes --watch
```

Publishing the source Release normally runs the complete native installation
matrix before dispatching the public release build. The public workflow must
fail when the matching source Release or its body is missing.

## Release gate exception

The native installation gate remains mandatory by default. When GitHub cannot
start the private-repository jobs because of a documented infrastructure or
billing failure, an operator may use the locally recorded release validation as
a one-time exception and dispatch the public workflow directly:

```bash
gh workflow run release.yml --repo apilots/talos-releases \
  -f tag=v0.1.8 \
  -f publish_latest=true
```

Before direct dispatch, confirm that the failed source workflow shows a billing
or infrastructure failure before product tests ran. Record that workflow URL,
the local commands and results, the exact source tag and SHA, and the directly
dispatched workflow URL in `output/release-<version>/release-evidence.md`. A local
macOS host cannot replace the unavailable Windows and Linux installation rows;
record those rows as unverified. This exception does not change or disable the
default source-Release gate.

When publishing consecutive versions, dispatch the older version with
`publish_latest=false`, wait until its public workflow and artifacts complete,
then dispatch the newest version with `publish_latest=true`. This prevents an
older run from moving stable `latest` references backward.

## Verify and finish

Verify that both repositories have byte-identical Release Notes for the tag.
Then download the public assets, run `sha256sum --check checksums.txt`, inspect
`manifest.json`, and inspect the server image for `linux/amd64` and `linux/arm64`.
For the newest release, also verify the stable latest references. A release is
incomplete while any required artifact or verification is missing.

After publishing, merge the release branch back into `main`, then start the next
development version in a separate `chore(release)` commit. Protocol or data
changes also require coordinated consumer rollout and the backup and rollback
procedure in [Deployment](../../deploy/README.md#data-upgrades-and-rollback).
