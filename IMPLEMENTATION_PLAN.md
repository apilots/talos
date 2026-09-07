# Local and Cloud Deployment Mode Plan

## Goal

Add a top-level deployment mode for instance-level LLM and web search configuration access:

```yaml
mode: local # local | cloud
```

Configuration management is allowed when authentication is disabled, the deployment is local, or the authenticated user is an administrator.

## Phase 1: Configuration model and deployment profiles

**Goal**: Add a strongly typed deployment mode with a safe local default and a dedicated Cloud profile.

**Success criteria**:
- Missing `mode` defaults to `local`.
- `talos-cloud.yaml` declares `mode: cloud` with authentication enabled.
- The server Docker image copies `talos-cloud.yaml` as its runtime configuration.
- The default `talos.yaml` remains unchanged.

**Tests**: Existing product configuration coverage validates each profile's expected deployment mode and the Cloud authentication requirement.

**Status**: Completed

## Phase 2: Backend authorization

**Goal**: Enforce the deployment-aware rule at the shared configuration API boundary.

**Success criteria**:
- LLM and web search configuration reads and writes share one authorization check.
- Local deployments and authentication-disabled deployments remain unrestricted.
- Authenticated Cloud users require the administrator role.
- Model listing and normal web search execution remain unchanged.

**Tests**: Gateway and HTTP tests cover allowed local access and denied regular Cloud access.

**Status**: Completed

## Phase 3: Web visibility

**Goal**: Hide instance configuration from regular Cloud users using the server deployment mode.

**Success criteria**:
- HTTP health exposes the deployment mode.
- Web stores it in a server-owned `deploymentMode` field.
- Models and Web Search navigation and direct routes use the backend-equivalent rule.
- The unused frontend `runtimeMode` setting and selector are removed.
- Desktop remains a local deployment without changing the App Server protocol.

**Tests**: Authentication bootstrap and settings routing tests cover local, Cloud user, Cloud administrator, and authentication-disabled behavior.

**Status**: Completed

## Phase 4: Integration validation

**Goal**: Verify configuration, authorization, and Web behavior through real and automated paths.

**Success criteria**:
- Rust formatting, scoped tests, and lint checks pass.
- Web tests, type checking, lint, and repository checks pass.
- A real server started with `talos-cloud.yaml` reports Cloud mode.
- A regular Cloud user receives permission denied and cannot see configuration navigation.
- A Cloud administrator can read and open configuration.

**Tests**: Run scoped Rust and Web checks plus the real Cloud login flow for both roles.

**Status**: Completed

## Scope decisions

- Cloud Computer provisioning and runtime selection are outside this change.
- TUI behavior is unchanged.
- Desktop App Server protocol is unchanged because Desktop is a local deployment.
- The HTTP authentication response-envelope fix is an independent prerequisite change and should be committed separately.
