# Talos Spec Organization

`spec/` is the decision and planning entrypoint for Talos changes. New work should start from a scoped spec directory before implementation when the change affects product behavior, architecture, runtime configuration, protocol shape, or cross-client behavior.

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

Small specs may contain only `README.md`. Larger specs should use:

```text
feat-001-short-name/
  README.md
  design.md
  implementation-plan.md
  test-plan.md
  decisions.md
```

Each spec `README.md` should include:

- Status: `draft | active | implemented | verified | archived`
- Background and problem
- Goals
- Non-goals
- Current implementation facts
- Design decisions
- Impacted areas
- Verification strategy
- Open risks or follow-ups

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

## AI Collaboration Rules

- Read the relevant active spec before changing code.
- If a decision is missing, update `decisions.md` or the spec README before encoding it in code.
- Keep implementation plans staged and verifiable.
- Record validation commands and results before marking a spec `verified`.
- Prefer concise, current specs over large historical documents. Historical material belongs in `archive/`.

## Legacy Specs

Root-level historical specs have been archived into typed directories under `spec/archive/2026/` based on filename and title. They are treated as already implemented historical references.
