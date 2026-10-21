# Talos Engineering Guide

Apply rules to the changed behavior. Check directory instructions when entering a new scope; `AGENTS.override.md` takes precedence within its directory. More specific instructions apply within their scope, subject to platform instructions and explicit user requirements.

## Core constraints

- Reuse the existing capability owner, Services, Stores, configuration, and event paths. Add an abstraction only for a current caller or stable boundary; avoid parallel managers, registries, protocols, and speculative extension points.
- Keep lifecycle, state, execution, transport, and presentation separate. Upper layers compose policy; lower layers consume stable inputs without depending on upper-layer services. Core crates own only stable shared capabilities; evaluate a more appropriate crate before expanding them.
- Keep one typed domain model across transports and clients. Use enums for business variants and structured persistence schemas. Store records and transport envelopes must not leak into public domain APIs; SDKs hide transport differences.
- Public APIs need clear ownership, permissions, errors, and reachable management paths. Document caller expectations, implementation constraints, extension conditions, and excluded responsibilities in English. Avoid arbitrary JSON, string conventions, and vague containers for core semantics.
- Preserve the unified `TalosError` / `ApiError` contract, typed sources, and one handling-owner report. Branch on codes and typed parameters, never public English messages; never expose diagnostic `Display` text to users or models.
- Fail explicitly at the responsible boundary. Do not conceal broken state with fallback paths or test-only production branches. Avoid compatibility code for unreleased behavior and flags that will never be disabled after release.

## Agent-first contribution contract

- AI coding agents are the default code implementers. Humans provide product intent, design direction, decisions, and review; they are not expected to write implementation code.
- Agents own repository inspection, scoped planning, implementation, relevant documentation, verification, and an evidence-based handoff. Follow the task's authorization boundary; never merge, publish, deploy, or use external credentials without authorization.
- Ask a human when product intent, public behavior, security boundaries, or acceptance criteria require a decision. Resolve routine implementation choices from repository conventions.
- Use the structured request and pull request fields in [CONTRIBUTING.md](CONTRIBUTING.md). Report exact checks and results; label unavailable or skipped checks with the reason.

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

- Keep actual self-test, review and acceptance records in the repository-root ignored `output/` directory, including result tables, verification-only Markdown, logs, screenshots and issue observations. Use `.tmp/` for disposable scratch files. Do not commit these records under specs or deployment documentation, including concise copies of the same results.
- Tracked specs and documentation own designs, contracts, reusable verification procedures and deployment/acceptance checklists. Summarize actual commands, results, limitations and blockers in PR evidence; link to PRs rather than ignored local reports when a durable reference is needed. Keep task plans and progress in the ignored task-owned `IMPLEMENTATION_PLAN.md` as required by the development guide.
- Use [scoped validation commands](docs/development/DEVELOPMENT.md#scoped-validation) from the repository root. Rust tests run through `just test -p <package>` (cargo-nextest), not bare `cargo test`. The repository check is `pnpm --dir talos-app check:repo`.
- Choose checks for the affected behavior. Fix failures caused by the change and continue through authorized implementation and verification; a first implementation or successful compile alone is not completion. Broaden or repeat checks only for changed behavior, failures, or unresolved risk.
- Review the requested diff for missing behavior and unnecessary code, fixtures, conversions, or workarounds. Remove obsolete paths introduced or replaced by the change; leave no TODO without an associated issue or plan. Preserve unrelated user work.
- Do not disable tests, lint, or hooks to pass checks. Report unavailable verification and its impact rather than claiming completion. If user input is required, finish independent authorized work and present the concrete unresolved decision.
- Preserve focus intent in application layouts: initial render, page refresh and passive responsive changes must not focus navigation or drawer toggles merely because `document.activeElement` is `body`, or open a narrow modal from a persisted wide-layout preference. Restore focus only when a user-driven close or hidden/removed focused surface requires it, and do not steal focus already moved elsewhere. Keep keyboard focus-visible indicators. When changing drawer/modal lifecycle, cover initial and refreshed pages plus genuine close/Escape focus recovery in the owning regression and representative browser scenario.

## E2E quality gates

- Agent capabilities and product behavior must have meaningful E2E coverage in the GitHub-hosted PR quality gate when they fit the supported runner environment.
- Real multi-control-plane, multi-machine, synchronization, or other unusually resource-intensive topologies may use local acceptance. Mark those scenarios explicitly in the task plan and E2E documentation, and report their local result separately; never silently skip them or label them as passing CI checks.
- Local-only topology acceptance does not replace focused gated coverage for the product behavior it exercises. Keep that behavior covered by a representative scenario that can run on GitHub-hosted runners.
