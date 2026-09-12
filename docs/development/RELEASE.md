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
creates an annotated tag and source Release. That Release records source and
Notes; downloadable products become available only after the public pipeline
passes. If tagging succeeds but source Release creation fails, rerun the same
command while the tag still points to HEAD and the source Release is absent.
Existing published tags must never be moved or rebuilt with changed code.

## One release gate

`apilots/talos-releases` executes this sequence for both repository dispatch and
manual workflow dispatch:

1. Resolve the tag to a source SHA; validate formal versions, preparation/fix
   history, source Release and nonempty canonical Notes.
2. Run `bash scripts/check-release.sh` from that source: commit and Notes contract
   tests, Rust format/check/tests, frontend pipeline, and installer contract tests.
3. Build products and images from the validated source SHA. Run native installation
   E2E on each of the four TUI products before uploading it, reusing its built bundle
   through `TUI_INSTALL_PRODUCT_DIR` rather than rebuilding it.
4. After all product builds and native checks pass, publish image tags and prepare
   final assets; validate asset contracts before publishing the artifact Release.
5. Compare the published source and artifact Notes. Complete the remaining
   download, manifest, and latest checks under Finish manually.

Talos owns the checks and E2E implementation. The public repository only provisions
runners and orchestrates them. Source-side dispatch does not duplicate the gate.
There is no skip-gate input or local-validation substitute. If private runners
cannot start the lightweight dispatch job, invoke the same public workflow directly:

```bash
gh workflow run release.yml --repo apilots/talos-releases \
  -f tag=v0.1.9 -f publish_latest=true
```

This changes only the trigger; all public gates remain mandatory. Publish consecutive
versions serially, using `publish_latest=false` for older versions. The public
workflow serializes release runs and prevents older versions from moving latest
backward relative to the current public GitHub latest release. Local bug fixes
need only affected scenario checks; the full suite runs once as the release gate.
Keep local verification output in ignored `output/release-<version>/`; record
unavailable external-account scenarios as unverified. Never infer their success
from unit tests or artifact builds.

## Finish

Download public assets and check `checksums.txt` using `sha256sum --check` on Linux
or `shasum -a 256 -c` on macOS. Verify byte-identical source/artifact Notes, manifest
source SHA, and both `linux/amd64` and `linux/arm64` image platforms. Verify latest
references for the newest release.

After successful publication, merge the release branch into `main`. Preserve main's
next development version when resolving version conflicts; never reset main to the
released version or copy the entire release tree over ongoing development. Update
Docker defaults to the newly published image. If main has not advanced yet, start
the next development version in a separate commit. Keep the released tag immutable.
Protocol/data upgrades also follow [Deployment](../../deploy/README.md#data-upgrades-and-rollback).
