# Contributing to Talos

[简体中文](CONTRIBUTING_zh.md)

Talos is built with AI coding agents as the default code implementers. Human
contributors provide product intent, design direction, decisions, and review;
they are not expected to write implementation code. Contributions from people
and agents are welcome when they follow the same repository contracts.

## Start with a decision-ready task

Use the **Agent task** issue form for new work. State the outcome, accepted
product/design decisions, constraints, non-goals, and observable acceptance
criteria. A task may be assigned directly to an agent instead of opened as an
issue, but preserve that same information in the prompt or plan.

The human owns product choices and approval boundaries. The agent owns repository
inspection, implementation, relevant tests and documentation, and an evidence-
based handoff. Ask for a decision only when unresolved ambiguity changes public
behavior, security, data, or product design; make routine implementation choices
from repository conventions.

## Agent workflow

1. Read the applicable `AGENTS.md` files and the referenced architecture or
   development contract before editing.
2. Inspect the current implementation and working tree. Preserve unrelated
   changes and identify the owning module, workflow, or document.
3. For staged work, maintain a task-owned `IMPLEMENTATION_PLAN.md` with scope,
   decisions, stages, success criteria, verification, and current status. Do not
   replace a plan owned by another task.
4. Implement the smallest change that meets the accepted criteria. Keep
   transport, lifecycle, state, execution, and presentation responsibilities in
   their existing owners.
5. Run the scoped checks required by repository guidance. Record the command and
   observed result in the pull request. Never report an unrun check as passing.
6. Use the pull request template to describe intent, implementation, decisions,
   risk, verification, and unresolved questions. Keep code and technical
   documentation in English unless the owning document has an established
   localization convention.

## CI and external contributions

- Fork pull request workflows require maintainer approval before they run. In
  **Settings → Actions → General**, set fork workflow approval to require
  approval for **all external contributors**; first-time-contributor-only approval
  is not sufficient for this policy. This is a repository setting and cannot be
  enforced by a workflow file alone.
- Public PR workflows use `pull_request`, read-only `GITHUB_TOKEN` permissions,
  and no repository secrets. Never move fork code into a privileged
  `pull_request_target` workflow or execute it with secrets.
- Secret-backed model and search-provider acceptance is available only through
  the maintainer-dispatched workflow on `main`. Private Cloud service-source
  scenarios remain local acceptance until their fixtures can be reproduced from
  public sources.
- Do not request, copy, or add personal access tokens for dependencies that are
  public or for release dispatch that the current repository workflow does not
  perform. Keep only credentials required by an active workflow, such as the
  release workflow's optional R2 publishing credentials.

Every pull request runs lightweight file-policy and change-scope checks.
Resource-heavy Rust, frontend, and business E2E jobs run only when product
content or the owning build/workflow inputs change. PRs and main pushes use the
same content-aware selector. Changes limited to the owned version fields and
release Notes run actual version-metadata and policy checks; mixing a dependency,
configuration or code change into those files still runs the affected product jobs.
Documentation, specifications, issue forms, and
pull request templates alone skip those jobs; product configuration such as
`conf/**` still runs the applicable checks. Heavy installation and distribution
architecture checks run only in release or
manual installation preflight; related PR changes run lightweight installer contracts.
Fixed required status checks remain
successful for out-of-scope changes so documentation-only pull requests can merge.

## Review standard

Review whether the change meets the accepted intent and contracts, not who typed
the code. The PR must make these facts easy to verify: what changed, why, which
decisions it follows, what was checked, what remains unverified, and whether a
human decision is still needed. Do not require a generic AI-authorship
declaration; this repository already treats agents as the expected implementers.
