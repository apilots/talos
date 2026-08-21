# Project structure

[Chinese](PROJECT_STRUCTURE_zh.md)

This document describes the current top-level repository layout. Detailed crate
inventory lives in [Rust workspace modules](RUST_MODULES.md).

```text
talos/
|-- .githooks/             Repository-managed Git hooks
|-- build/                 Packaging, release, branding, and product resources
|-- conf/                  Shared configuration and database migrations
|-- deploy/                Server deployment and TUI installation assets
|-- docs/
|   |-- architecture/      Current cross-component architecture
|   |-- development/       Development workflow
|   `-- reference/         Current repository and workspace facts
|-- scripts/               Repository automation and quality checks
|-- spec/                  Active and archived change specifications
|-- talos-app/
|   |-- desktop/           Electron Desktop shell
|   |-- ts-sdk/            Core, browser, and app-server TypeScript SDKs
|   `-- web/               React Web UI
|-- talos-e2e/
|   |-- docker/            E2E container resources
|   |-- scenarios/         Executable scenarios grouped by product flow
|   |-- scripts/           Shared E2E helpers
|   `-- tests/             Runner and helper tests
|-- talos-rs/
|   |-- apps/              Rust SDKs, TUI, and protocol test client
|   |-- bin/               Product and protocol-adapter binaries
|   `-- crates/            Layered Rust libraries
|-- AGENTS.md              Repository engineering rules
|-- CLAUDE.md              Claude-specific repository guidance
|-- README.md              English product and repository entry point
|-- README_zh.md           Chinese product and repository entry point
`-- justfile               Canonical local development commands
```

## Sources of truth

| Fact | Source |
| --- | --- |
| Rust workspace membership | `talos-rs/Cargo.toml` |
| JavaScript workspace membership | `talos-app/pnpm-workspace.yaml` |
| Common development commands | Root `justfile` |
| Active and archived change designs | `spec/REGISTRY.md` |
| E2E scenario discovery | `talos-e2e/run_e2e.sh` and `talos-e2e/scenarios/` |
| Deployment topology | `deploy/` assets and README |

Generated outputs such as `target/`, `dist/`, reports, logs, and test results are
not part of the maintained source layout.
