# Public Cloud SDK integration verification

Date: 2026-10-13. Scope: the existing Talos Cloud authentication adapter and its
build inputs. No authentication policy, service URL, Sync protocol or production
service configuration changed.

## Dependency and ownership

Talos now uses `talos-cloud-sdk = "=0.1.3"` from crates.io. The existing adapter
imports `CloudSdkError`, `TalosCloudClient` and the SDK's `talos_cloud_types`
re-export; no direct types dependency, old package alias, path or patch remains.
The original origin-only API URL and `with_access_token` calls are preserved.

Rust quality/native-install CI, Server/Worker images and SDK-only sandbox/install
containers use the published Cloud SDK and do not require Cloud source in their
build contexts. The independent Sync server also consumes the public Cloud SDK;
its product image and the Talos 19-group runner require no private SDK bundle or
named context. Isolated authentication E2E launches a separately supplied fixture
executable using `TALOS_CLOUD_SERVER_BIN` (or `cloud-server` on `PATH`); it does
not require a Cloud source checkout.

## Observed checks

- `just test -p talos-auth`: 17/17 passed, 0 skipped (6.859 seconds).
- `just clippy -p talos-auth -- -D warnings`: passed.
- `TALOS_E2E_CADDY_BIN=/tmp/talos-e2e-tools/caddy just e2e-case 18_computer_workspace/01_registration`:
  passed (81.31 seconds; run `20261013_134101`), including ten genuine device login, protected cache,
  heartbeat/renewal, UUID restart, credential recovery, owner/account isolation
  and logout checks.
- Cargo metadata/tree: SDK and types both resolve to registry 0.1.3; no old SDK
  package or adjacent private production path.
- Workflow YAML parsing, changed shell syntax, existing TUI target test and diff
  whitespace checks passed. The Sync project's own verification records its
  public dependency and standalone image checks.

The previously completed full eight-case Computer Sync acceptance was not rerun.
Fresh product Docker builds and remote CI are left to the normal delivery/release
checks; this scope does not claim their results or deploy production.

Transient scenario reports and registry metadata are preserved locally under
`output/validation/feat-044-talos-sync/20261013-precommit-cleanup/` and are not
committed with the specification.
