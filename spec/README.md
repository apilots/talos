# Talos Spec Organization

`spec/` is the technical design entrypoint for Talos changes. New work should start from a scoped spec directory before implementation when the change affects product behavior, architecture, runtime configuration, protocol shape, or cross-client behavior.

## Directory Model

New specs must use typed, incrementing directories:

```text
spec/
  README.md
  REGISTRY.md
  feat-001-short-name/
  fix-001-short-name/
  refactor-001-short-name/
  docs-001-short-name/
  archive/
    2026/
      refactor-001-short-name/
```

Types:

- `feat-*`: New product capability or user-visible behavior.
- `fix-*`: Confirmed bug fix. Must record root cause, reproduction, and verification.
- `refactor-*`: Architecture, module boundary, runtime, config, or protocol cleanup where the main goal is structural quality.
- `docs-*`: Documentation, process, spec governance, or non-product documentation work.

Rules:

- IDs are independent per type and never reused.
- IDs are three digits: `001`, `002`, `003`.
- Directory suffixes use English kebab-case.
- Before creating a spec, read and update `REGISTRY.md`.
- Existing archived IDs still count as used IDs.

## Spec Contents

Each active spec directory should keep a single core technical design document:

```text
feat-001-short-name/
  design.md
```

`design.md` should include:

- Status: `draft | active | implemented | verified | archived`
- Background and problem
- Goals
- Non-goals
- Current implementation facts
- Design decisions
- Impacted areas
- Verification strategy
- Open risks or follow-ups

Do not put execution breakdowns in `spec/`. Implementation staging, task ownership, and verification progress belong to the separate execution `PLAN` used during implementation.

## Lifecycle

Specs move through:

```text
draft -> active -> implemented -> verified -> archived
```

Archive a spec only when:

- The implementation is merged, completed, or explicitly abandoned.
- Verification results are recorded.
- Remaining work is moved to a new spec, issue, or clearly marked out of scope.
- The spec is no longer the active entrypoint for current work.

Archived specs live under `spec/archive/<year>/` and keep their original typed ID. Do not edit archived specs for new work. Create a new `feat-*`, `fix-*`, or `refactor-*` spec and reference the archived one.

The active set is intentionally small. `REGISTRY.md` is the authoritative index; directories under `archive/` are historical context and must not be treated as current design requirements.

## AI Collaboration Rules

- Read the relevant active spec before changing code.
- If a technical decision is missing, update the spec `design.md` before encoding it in code.
- Keep implementation plans outside `spec/` in the active execution `PLAN`.
- Record final validation evidence in the execution output or implementation `PLAN`, not in the design spec.
- Prefer concise, current specs over large historical documents. Historical material belongs in `archive/`.

## Archived Specs

All superseded specs have been moved into typed directories under `spec/archive/2026/`. Their `archived` status means they are no longer active design entrypoints; it does not retroactively claim that every draft was implemented.
