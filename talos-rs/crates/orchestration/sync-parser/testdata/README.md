# Released schema-v1 fixtures

`released-v1/` contains full Loro snapshots produced by the immutable published
`talos-sync-sdk = 0.1.7`, plus its decoded changes and exact version vectors.
They exercise conversion through the current Talos Parser and real Store
migration/rebinding. Production depends only on SDK 0.1.8.

- `source-a`: two original A Turns.
- `survivor-b`: the same operations plus two original B Turns.
- `materialized-b`: an exported, unacknowledged local edit followed by a newer
  remote edit; replaying the old outbox must not overwrite the remote value.
- `deleted`: a reset, next-generation history and terminal deletion.
- `pending.json`: an unmaterialized native metadata edit for mixed-tail tests.
- `envelopes`: original Active/Command/Notice records, including a valid missing
  optional field preserved as raw JSON rather than filled by a current DTO.

Each Turn contains the native six-event message lifecycle. Original source IDs,
occurrence/persistence times, counters and dependencies are part of the fixture.

## Regeneration

Use a disposable standalone Cargo project outside the production workspace.
First fetch the exact registry SDK and find its package manifest; then seed the
generator lockfile from the lockfile shipped in that package. This preserves its
Loro/internal dependency versions instead of resolving a later semver-compatible
internal package with an incompatible API.

From the repository root:

```sh
mkdir -p .tmp/released-sync-fixture/src
cat > .tmp/released-sync-fixture/Cargo.toml <<'TOML'
[package]
name = "released-sync-fixture"
version = "0.0.0"
edition = "2021"
publish = false
[workspace]
[dependencies]
talos-sync-sdk = "=0.1.7"
serde_json = "1"
loro = { version = "=1.16.2", default-features = false }
TOML
cp talos-rs/crates/orchestration/sync-parser/testdata/generate_v1.rs \
  .tmp/released-sync-fixture/src/main.rs
cargo metadata --manifest-path .tmp/released-sync-fixture/Cargo.toml \
  --format-version 1 > .tmp/released-sync-fixture/metadata.json
python3 - <<'PY'
import json
import shutil
from pathlib import Path
root = Path('.tmp/released-sync-fixture')
packages = json.loads((root / 'metadata.json').read_text())['packages']
sdk = next(p for p in packages if p['name'] == 'talos-sync-sdk' and p['version'] == '0.1.7')
shutil.copyfile(Path(sdk['manifest_path']).parent / 'Cargo.lock', root / 'Cargo.lock')
PY
cargo run --manifest-path .tmp/released-sync-fixture/Cargo.toml -- \
  talos-rs/crates/orchestration/sync-parser/testdata/released-v1
cargo run --manifest-path .tmp/released-sync-fixture/Cargo.toml -- \
  talos-rs/crates/orchestration/sync-parser/testdata/released-v1 envelopes
just test -p talos-sync-parser -p talos-store \
  -E 'test(upgrade::tests::) or test(released_upgrade_tests::)'
```

Do not introduce a production dependency, patch or runtime dual-codec path for
the released SDK. Snapshot output can contain Loro timestamps and need not be
byte-identical after regeneration; the tests check native facts and causal merge
behavior.
