# Talos Engineering Guide

Apply rules to the changed behavior. Check directory instructions when entering a new scope; `AGENTS.override.md` takes precedence within its directory. More specific instructions apply within their scope, subject to platform instructions and explicit user requirements.

## Core constraints

- Reuse the existing capability owner, Services, Stores, configuration, and event paths. Add an abstraction only for a current caller or stable boundary; avoid parallel managers, registries, protocols, and speculative extension points.
- Keep lifecycle, state, execution, transport, and presentation separate. Upper layers compose policy; lower layers consume stable inputs without depending on upper-layer services. Core crates own only stable shared capabilities; evaluate a more appropriate crate before expanding them.
- Keep one typed domain model across transports and clients. Use enums for business variants and structured persistence schemas. Store records and transport envelopes must not leak into public domain APIs; SDKs hide transport differences.
- Public APIs need clear ownership, permissions, errors, and reachable management paths. Document caller expectations, implementation constraints, extension conditions, and excluded responsibilities in English. Avoid arbitrary JSON, string conventions, and vague containers for core semantics.
- Preserve the unified `TalosError` / `ApiError` contract, typed sources, and one handling-owner report. Branch on codes and typed parameters, never public English messages; never expose diagnostic `Display` text to users or models.
- Fail explicitly at the responsible boundary. Do not conceal broken state with fallback paths or test-only production branches. Avoid compatibility code for unreleased behavior and flags that will never be disabled after release.

## Read when relevant

These are task-triggered references, not a startup reading list. Read the linked section before changing that behavior; add other references only when the task crosses their boundary. A typo or prose edit does not require reading implementation contracts merely because the document mentions them.

| Task touches | Read |
| --- | --- |
| Rust implementation or tests | [Rust conventions](docs/development/RUST.md) |
| Public DTOs, JSON-RPC, wire shape, or compatibility | [API conventions](docs/development/API.md) |
| Error construction, propagation, presentation, or replay | [Error contract](docs/architecture/ERROR_MODEL.md) |
| Configuration or persistent state | [State rules](docs/architecture/RUNTIME_ARCHITECTURE.md#state-rules) |
| MCP connection lifecycle, configuration, or tool mutation | [MCP implementation rules](docs/architecture/RUNTIME_ARCHITECTURE.md#mcp-implementation-rules) |
| Talos model context construction or injection | [Context rules](docs/architecture/RUNTIME_ARCHITECTURE.md#context-rules) |
| Product UI, TUI styling, or runtime text | [UI implementation](docs/architecture/CLIENT_ARCHITECTURE.md#ui-implementation) |
| Behavior changes and their tests | [Behavioral verification](docs/development/DEVELOPMENT.md#behavioral-verification) |
| Staged work or substantial refactoring | [Plans and durable evidence](docs/development/DEVELOPMENT.md#plans-and-durable-evidence) |
| Configuration/API schema, dependencies, or compile-time resources | [Generated artifacts](docs/development/DEVELOPMENT.md#generated-artifacts) |

For unfamiliar ownership, consult the relevant entry in [Rust modules](docs/reference/RUST_MODULES.md) or [Project structure](docs/reference/PROJECT_STRUCTURE.md). There is no requirement to map the whole repository before editing.

## Validation and completion

- Keep transient test/review logs, screenshots, reports, and scratch files in the repository-root ignored `output/` or `.tmp/` directories. Specs retain designs, plans, and concise verification conclusions, not process artifacts.
- Use [scoped validation commands](docs/development/DEVELOPMENT.md#scoped-validation) from the repository root. Rust tests run through `just test -p <package>` (cargo-nextest), not bare `cargo test`. The repository check is `pnpm --dir talos-app check:repo`.
- Choose checks for the affected behavior. Fix failures caused by the change and continue through authorized implementation and verification; a first implementation or successful compile alone is not completion. Broaden or repeat checks only for changed behavior, failures, or unresolved risk.
- Review the requested diff for missing behavior and unnecessary code, fixtures, conversions, or workarounds. Remove obsolete paths introduced or replaced by the change; leave no TODO without an associated issue or plan. Preserve unrelated user work.
- Do not disable tests, lint, or hooks to pass checks. Report unavailable verification and its impact rather than claiming completion. If user input is required, finish independent authorized work and present the concrete unresolved decision.
