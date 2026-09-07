# Talos documentation

[Chinese](README_zh.md)

This directory contains current, cross-component documentation. Baseline: the
Provider, unified error and deployment-mode changes merged by 2026-09-07.
Change-specific designs remain under [`spec/`](../spec/) and are outside this hierarchy.

Read README -> System -> Domain for orientation; then Execution/Memory for
backend work, Client/Error for integrations, or Deployment/Development/Release
for operating and delivering the product. Each topic owns its detailed contract.

## Architecture

- [System architecture](architecture/ARCHITECTURE.md): system boundaries and the overall architecture
- [Domain model](architecture/DOMAIN_MODEL.md): identity, Workspace scope, binding, and lifecycle vocabulary
- [Runtime architecture](architecture/RUNTIME_ARCHITECTURE.md): session, turn, agent, subagent, and tool execution
- [Memory architecture](architecture/MEMORY_ARCHITECTURE.md): source documents, projections, reflection, and context limits
- [Client architecture](architecture/CLIENT_ARCHITECTURE.md): Web, Desktop, TUI, SDK, and gateway boundaries
- [Error model](architecture/ERROR_MODEL.md): internal errors and protocol-safe error projection

## Reference

- [Deployment](../deploy/README.md): local/cloud configuration access, safety, and upgrade boundaries
- [Project structure](reference/PROJECT_STRUCTURE.md): current repository layout
- [Rust workspace modules](reference/RUST_MODULES.md): current Rust workspace inventory

## Development

- [Development workflow](development/DEVELOPMENT.md): tools, checks, tests, and generated artifacts
- [Release workflow](development/RELEASE.md): version validation, tagging, and release automation

## Documentation rules

- English files are canonical and use the normal `.md` name.
- Chinese counterparts use the `_zh.md` suffix.
- New and revised contracts are maintained in English. Older translations may
  remain as historical reading aids with an explicit link to the current source;
  reference-only entries such as `ERROR_MODEL_zh.md` do not define another contract.
- Architecture diagrams use plain ASCII inside `text` code fences.
- Preserve useful diagrams when editing. Label ownership maps versus execution
  sequences, distinguish current/planned capabilities, and keep connectors aligned.
- Current-state reference documents must be checked against source manifests.
- Component details stay in the closest component README.
- Historical and change-specific decisions stay in `spec/`; do not duplicate them here.
- `IMPLEMENTATION_PLAN.md` is local and ignored. Keep durable design decisions and
  verification references in tracked specs or PR evidence, not only in that file.
