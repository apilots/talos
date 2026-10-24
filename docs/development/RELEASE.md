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
file list and validates both values and preparation history, including the local
workspace package versions in the committed `talos-rs/Cargo.lock`. After changing
workspace versions, run `cargo metadata --offline --format-version 1
--manifest-path talos-rs/Cargo.toml > /dev/null` to update those lock entries;
review that registry versions, sources and checksums remain unchanged. Include
the lockfile in the standalone version commit. Version-only lock changes skip
heavy PR/main jobs; dependency graph changes still trigger the affected checks.

Commit and review the lockfile with every dependency change. CI fetches the
committed graph with `cargo fetch --locked`; it never regenerates the graph.

After branching, `main` may immediately advance to the next `-SNAPSHOT` version
in a separate `chore(release): start v<next-version> development` commit. Keep
Docker defaults on the latest published formal image until the new release ships.
Development does not wait for the release branch to finish.

General release policy and automation changes land on `main` for future versions;
do not backport them to an existing release branch as product bug fixes.

## Stabilize

Each push to `release-*` runs the full source gate once in `release-source.yml`.
Its visible stages cover contracts, Rust formatting/check/tests, frontend quality
and installer contracts. It does not build development product packages. Candidate
runs are isolated per branch; superseded candidates cancel, while publication runs
remain serialized. Release runner labels and Rust/Node/pnpm versions are fixed in
`.github/release-toolchain.env`; update them through reviewed PRs.

A successful candidate preserves its exact source SHA, run/attempt, preceding
published tag, Cargo.lock, toolchain fingerprint and standard generated Notes for
14 days. The tag workflow verifies and reuses that source gate and graph; it does
not rerun the source suite or resolve a new dependency graph. Missing, expired or
mismatched evidence fails explicitly. Rerun the candidate gate at the same source
when evidence is unavailable, then review its evidence before tagging. The
candidate must retain the exact committed lockfile; recording fails if source
checks changed it. Formal product builds and artifact checksum checks continue
to use that same validated graph.

The release branch accepts only bug fixes, including necessary regression tests
and corrections to release documentation or packaging. Use `fix` subjects after
the preparation commit; no features, unrelated refactors, or merges from advancing
`main`. Transfer individual fixes with cherry-pick when necessary. The version
commit need not be HEAD: the final tag points to the fully corrected release tip.
The shared validator rejects non-fix commits after preparation. Commit classification
is a guardrail; review must still ensure that a fix does not introduce a feature.

The candidate generates English Notes with the existing
`scripts/generate-release-notes.sh`, from the preceding published tag to the exact
candidate SHA. Keep its commit-type sections and standard content. The same bytes
become the local reviewed file, annotated tag and public Release body. Generic
upgrade/rollback instructions belong in deployment documentation.

```bash
./release.sh 0.1.10 --dry-run --yes
# Review output/release-notes/v0.1.10.md, then publish the same candidate.
./release.sh 0.1.10 --yes
```

The branch must be clean, synchronized and backed by a successful candidate gate.
`--notes <file>` is optional and must equal that candidate's generated Notes.
The script verifies release absence and candidate provenance before pushing the
annotated tag with verbatim text. Tag pre-push checks validate the annotation,
source commit and version without repeating branch source checks; the publisher
and tag workflow still require the successful exact-source candidate. It performs
no product build or local package
inspection. It never corrects tags or adds a publication recovery path.

Before the first publication from a repository, or after moving the publishing
repository, inspect **Manage Actions access** on existing GHCR packages and grant
the publishing repository **Write**. Registry login and package metadata reads do
not prove this permission; GitHub provides no equivalent Actions ACL read API.
Keep the workflow's normal GITHUB_TOKEN and avoid fake uploads or personal-token
workarounds. Check release immutability settings and finalize Notes before publish:
immutable release tags/assets are locked, even when only their annotation changes.

## One release gate

The `release.yml` workflow in `apilots/talos` runs this sequence when a `v*` tag
is pushed, or when a maintainer manually retries an existing tag:

1. Resolve the immutable tag to a SHA and validate version/history and annotation.
2. Verify the successful release-branch candidate for that exact SHA, run/attempt,
   Notes, lock and toolchain. Import its source evidence without repeating tests.
3. Build each of the seven formal product targets once using the validated lock.
   Verify the build preserved that graph. Check the actual packages before upload;
   each TUI's native acceptance checks and installs that same built bundle through
   `TUI_INSTALL_PRODUCT_DIR`. No second development package is made for verification.
4. Download the same run's Linux AMD64 TUI product and run Ubuntu container
   installation through `release-install.yml`. The container has no Rust toolchain
   or build volume; missing products and failed installation block image publication
   and release publication.
5. Assemble Server/Worker runtime images from the validated Linux Server packages
   for each architecture, reusing their binaries, launcher and WebUI. The Docker
   `PRODUCT_STAGE=packaged` input selects this normal release path; the default
   `source` stage remains available to local source-image builds. Runtime dependency,
   version, launcher and license probes remain in the images before upload.
6. Publish the multi-architecture manifests after all product and image gates pass;
   validate final installer assets and create the GitHub Release with exact tag Notes.
   Preserve Cargo.lock and source-validation.json as checksummed public assets;
   manifest.validation records the candidate run and lock hash. Optional R2 remains.

PR/main run lightweight installer shell/archive contracts only. Platform business
E2E remains in affected module gates. For an unpublished installation preflight,
dispatch `release-preflight.yml` with `source_ref`: it resolves one immutable SHA,
builds and installs the three native TUI targets, then reuses its Linux AMD64
product for container acceptance without publishing anything.

All release jobs use GitHub-hosted runners with bounded durations. There is no
skip-gate input, unvalidated-result fallback or automatic tag repair. To retry an
incomplete publication at its unchanged tag, dispatch a fresh workflow run:

```bash
gh workflow run release.yml --repo apilots/talos \
  -f tag=v0.1.10 -f publish_latest=true
```

A completed GitHub Release is rejected before builds; this is not a way to rebuild
an existing version. Local release automation edits require scoped contract and
workflow/configuration checks, not full local product rebuilds. When changing
packaging behavior, choose relevant checks for that behavior; CI retains the final
package and native installation gates. Docker configuration can be checked with
`docker buildx build --check` without building an image.

Every product archive and Desktop package carries the Talos `LICENSE`,
`NOTICE`, and `THIRD_PARTY_NOTICES`; Server and Worker images install them under
`/usr/share/licenses/talos`. The release job verifies package contents before
upload. The third-party notice identifies dependency license families and
versioned source locations, including MPL-covered source. The GitHub Release
also includes these files alongside checksums and product artifacts.

Publish consecutive versions serially, using `publish_latest=false` for older
versions. The workflow serializes release runs and prevents older versions from
moving latest backward relative to the current GitHub latest release. Local bug
fixes need only affected scenario checks; each exact-source candidate runs the
source suite once and formal publication reuses its validated result.
Keep local verification output in ignored `output/release-<version>/`; record
unavailable external-account scenarios as unverified. Never infer their success
from unit tests or artifact builds.

## Supported release targets

Publish Desktop for macOS Apple Silicon and Linux AMD64, Server WebUI for Linux
AMD64/ARM64, and TUI for Linux AMD64/ARM64 and macOS Apple Silicon. macOS Intel
builds and native installation CI are disabled.

Final assembled-asset validation uses the tooling from the exact workflow commit
so its supported-target policy matches the build matrix. Product source, installer,
Notes and dependency provenance still come from the immutable release tag.

## Finish

Download public assets and check `checksums.txt` using `sha256sum --check` on Linux
or `shasum -a 256 -c` on macOS. Verify the release Notes match the annotated tag,
the manifest source SHA matches the immutable tag, and both `linux/amd64` and
`linux/arm64` image platforms are present. Verify latest references for the newest release.

After successful publication, archive the exact tag Notes in
`docs/releases/<tag>.md` without rewriting them, then merge the release branch
into `main`. Preserve main's
next development version when resolving version conflicts; never reset main to the
released version or copy the entire release tree over ongoing development. Update
Docker defaults to the newly published image. If main has not advanced yet, start
the next development version in a separate commit. Keep the released tag immutable.
Protocol/data upgrades also follow [Deployment](../../deploy/README.md#data-upgrades-and-rollback).
