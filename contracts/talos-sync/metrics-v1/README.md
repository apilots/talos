# Sync Metrics v1 contract snapshot

These canonical DTO/schema files are owned by the independent talos-sync server.
The manifest identifies their exact source paths and SHA256 contents. They do not
claim a published service revision; the source feature is locally verified and
uncommitted as of 2026-10-14. Review/merge and normal release gates remain required.

Refresh explicitly from an authorized source checkout, then review both contracts:

```sh
python3 scripts/sync-metrics-contract.py --source /path/to/talos-sync
python3 scripts/sync-metrics-contract.py --check
```

CI checks the committed snapshot and generated Rust decoder without requiring a
private/sibling repository. `--source /path/to/talos-sync --check` also verifies
exact cross-repository identity. The generator selects only the canonical account
DTO and its dependencies, dropping the schema-only derive for the private decoder.
Product APIs use Talos-owned types and explicitly convert milliseconds to seconds.
