# Talos documentation

[Chinese](README_zh.md)

This directory contains current, cross-component documentation. Change-specific
designs remain under [`spec/`](../spec/) and are outside this hierarchy.

## Architecture

- [System architecture](architecture/ARCHITECTURE.md): system boundaries and the overall architecture
- [Runtime architecture](architecture/RUNTIME_ARCHITECTURE.md): session, turn, agent, subagent, and tool execution
- [Client architecture](architecture/CLIENT_ARCHITECTURE.md): Web, Desktop, TUI, SDK, and gateway boundaries
- [Error model](architecture/ERROR_MODEL.md): internal errors and protocol-safe error projection

## Reference

- [Project structure](reference/PROJECT_STRUCTURE.md): current repository layout
- [Rust workspace modules](reference/RUST_MODULES.md): current Rust workspace inventory

## Development

- [Development workflow](development/DEVELOPMENT.md): tools, checks, tests, and generated artifacts
- [Release workflow](development/RELEASE.md): version validation, tagging, and release automation

## Documentation rules

- English files are canonical and use the normal `.md` name.
- Chinese counterparts use the `_zh.md` suffix.
- Architecture diagrams use plain ASCII inside `text` code fences.
- Current-state reference documents must be checked against source manifests.
- Component details stay in the closest component README.
- Historical and change-specific decisions stay in `spec/`; do not duplicate them here.
