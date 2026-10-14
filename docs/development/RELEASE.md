# Release workflow

[Chinese](RELEASE_zh.md)

Talos owns version tags, canonical Release Notes, build checks, product artifacts,
container images, and GitHub Releases. Existing releases and download assets in
`apilots/talos-releases` remain available as historical archives; new releases do
not depend on that repository.

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

## Open the release branch

Fetch remote `main` and create `release-<version>` from its exact commit. Immediately
change all release version locations to the formal version in one standalone
`chore(release): prepare v<version>` commit, then push the branch. Do not wait until
publication to change the version.

Version locations are the Rust workspace, TUI display, seven Web/Desktop/SDK
package manifests, App Server SDK default client metadata, and Docker example
and Compose image defaults. `scripts/check-release-version.sh` owns the exact
file list and validates both values and preparation history. Cargo.lock remains
ignored and generated from the workspace manifest.

After branching, `main` may immediately advance to the next `-SNAPSHOT` version
in a separate `chore(release): start v<next-version> development` commit. Keep
Docker defaults on the latest published formal image until the new release ships.
Development does not wait for the release branch to finish.

General release policy and automation changes land on `main` for future versions;
do not backport them to an existing release branch as product bug fixes.

## Stabilize

The release branch accepts only bug fixes, including necessary regression tests
and corrections to release documentation or packaging. Use `fix` subjects after
the preparation commit; no features, unrelated refactors, or merges from advancing
`main`. Transfer individual fixes with cherry-pick when necessary. The version
commit need not be HEAD: the final tag points to the fully corrected release tip.
The shared validator rejects non-fix commits after preparation. Commit classification
is a guardrail; review must still ensure that a fix does not introduce a feature.

Generate and edit English Notes from the preceding published tag to the final
release tip. The source repository owns the only canonical Notes; the artifact
repository copies them unchanged. The generator groups by commit type, excludes
merge, release preparation, and next-development version commits, keeps legacy untyped subjects under Other
Changes, rejects unknown typed subjects, and omits empty categories.

```bash
scripts/generate-release-notes.sh v0.1.9 --from v0.1.8 --output /tmp/talos-v0.1.9.md
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --dry-run --yes
./release.sh 0.1.9 --notes /tmp/talos-v0.1.9.md --yes
```

The branch must be clean and synchronized with its matching remote. The script
creates and pushes an annotated tag whose message is the canonical Release Notes.
Pushing the tag starts Talos's release workflow. It publishes the GitHub Release
only after source validation, quality checks, product builds, native TUI installation
checks, and image publication succeed. If a workflow run fails, rerun `release.yml`
for the same immutable tag after fixing the workflow or runner issue; never move a
published tag or rebuild it from changed code.

## One release gate

The `release.yml` workflow in `apilots/talos` runs this sequence when a `v*` tag
is pushed, or when a maintainer manually retries an existing tag:

1. Resolve the immutable tag to a source SHA; validate formal versions,
   preparation/fix history, and nonempty canonical Notes stored in the annotated tag.
2. Run `bash scripts/check-release.sh` from that source: commit and Notes contract
   tests, Rust format/check/tests, frontend pipeline, and installer contract tests.
3. Build the existing Desktop, WebUI, and four-platform TUI products. Run native
   installation E2E on each TUI product before uploading it, reusing its built bundle
   through `TUI_INSTALL_PRODUCT_DIR`.
4. Build and publish Server and Worker images for `linux/amd64` and `linux/arm64`;
   prepare checksums and the release manifest, then validate the final installer assets.
5. Create the Talos GitHub Release only after every required job succeeds, using the
   tag's canonical Notes unchanged. Optional R2 mirroring remains available.

All release jobs use GitHub-hosted runners. There is no skip-gate input or local
validation substitute. To retry the workflow for an existing tag:

Every product archive and Desktop package carries the Talos `LICENSE`,
`NOTICE`, and `THIRD_PARTY_NOTICES`; Server and Worker images install them under
`/usr/share/licenses/talos`. The release job verifies package contents before
upload. The third-party notice identifies dependency license families and
versioned source locations, including MPL-covered source. The GitHub Release
also includes these files alongside checksums and product artifacts.

```bash
gh workflow run release.yml --repo apilots/talos \
  -f tag=v0.1.9 -f publish_latest=true
```

Publish consecutive versions serially, using `publish_latest=false` for older
versions. The workflow serializes release runs and prevents older versions from
moving latest backward relative to the current GitHub latest release. Local bug
fixes need only affected scenario checks; the full release suite runs once as the gate.
Keep local verification output in ignored `output/release-<version>/`; record
unavailable external-account scenarios as unverified. Never infer their success
from unit tests or artifact builds.

## Finish

Download public assets and check `checksums.txt` using `sha256sum --check` on Linux
or `shasum -a 256 -c` on macOS. Verify the release Notes match the annotated tag,
the manifest source SHA matches the immutable tag, and both `linux/amd64` and
`linux/arm64` image platforms are present. Verify latest references for the newest release.

After successful publication, merge the release branch into `main`. Preserve main's
next development version when resolving version conflicts; never reset main to the
released version or copy the entire release tree over ongoing development. Update
Docker defaults to the newly published image. If main has not advanced yet, start
the next development version in a separate commit. Keep the released tag immutable.
Protocol/data upgrades also follow [Deployment](../../deploy/README.md#data-upgrades-and-rollback).
