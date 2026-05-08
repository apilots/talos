# Runtime Env And First-Run Bootstrap Refactor

Status: draft
Owner: agent
Created: 2026-05-08
Updated: 2026-05-08

## Background

Talos currently has split environment loading behavior across Rust entrypoints and Desktop. Rust loads only the process/current-working-directory dotenv path, while Desktop separately parses `~/.talos/.env`, invokes login shell environment loading, scans config placeholders, and passes merged values into the sidecar. This makes first-run behavior hard to reason about and can block packaged Desktop startup when `talos.yaml` or provider API keys are missing.

## Goals

- Make first-run startup independent from provider API key readiness.
- Generate `~/.talos/conf/talos.yaml` from an embedded template when missing.
- Generate and persist local secrets such as `TALOS_GATEWAY_JWT_SECRET` into `~/.talos/.env`.
- Keep provider API keys out of `talos.yaml`; persist user-entered keys into `~/.talos/.env`.
- Use one environment loading model for server, app-server, TUI, CLI, and Desktop sidecar.

## Non-Goals

- Do not add a separate OS installer flow beyond normal DMG installation.
- Do not store provider API keys in plaintext `talos.yaml`.
- Do not keep Desktop-specific shell environment discovery as a product dependency.

## Current Facts

- `Config::load()` creates base Talos directories but fails when `conf/talos.yaml` is missing.
- `validate_runtime()` currently treats missing or unresolved provider API keys as startup errors.
- `YamlLlmConfigStore` writes the full `llm` section back to YAML, including `api_key` values.
- Desktop has its own `.env` parser and login shell merge path.

## Design Direction

- Move authoritative env loading into `talos_config`.
- Load sources in this order: process environment > `~/.talos/.env` > `talos.yaml`.
- Reserve `TALOS__...` for structured config overrides.
- Use `TALOS_...` names for runtime secrets, for example `TALOS_LLM_ANTHROPIC_API_KEY` and `TALOS_GATEWAY_JWT_SECRET`.
- Split startup validation from model readiness validation.
- Default `gateway.auth=true` and `allow_register=true`; first registered user remains admin.
- After login, guide users to choose provider/model and save API keys into `~/.talos/.env`.

## Verification Strategy

- Empty `TALOS_HOME` starts app-server without panic and creates template files.
- Desktop packaged app opens registration/login on a fresh home.
- Missing provider API key disables chat/model readiness but does not block Settings.
- Saving a provider key updates `.env`, leaves YAML with placeholder references, and enables model usage after reload.
