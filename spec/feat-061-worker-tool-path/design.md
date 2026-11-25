# Worker tool PATH resolution

Status: `draft` — implementation and acceptance pending

Issue: #197

## Goal and scope

Grep and Bash resolve commands against the environment of the **Worker that executes the tool**, even when the Worker inherits a minimal PATH. The Worker automatically discovers its execution environment on macOS and Linux, regardless of whether it runs embedded, standalone, locally, or remotely. macOS Desktop launched from Finder is a regression scenario, not an implementation branch.

Grep uses `rg` on the executing Worker. The official Server and Worker Docker images already install `ripgrep` at image build time. Both identify their packaging at Worker startup and check `rg --version` at image build time as the runtime user. A missing dependency in those images is an image or PATH defect, not an install request. Talos does not bundle a separate `rg` executable with native distributions. A native macOS or Linux Worker can offer on-demand installation after explicit user approval.

This work covers Worker PATH discovery and recovery of Grep's `rg` dependency. Windows, built-in Browser MCP, StdIO MCP/plugin environment, Glob, and changes to sandbox policy are out of scope.

## Ownership and discovery

- Keep discovery and PATH assembly in an independent Rust module on the existing Worker tool-runtime path. Expose one internal interface to the runtime, with OS-specific discovery behind it. Reuse the same module for all Worker launch modes; do not add a Desktop probe, bootstrap field, or client-to-Worker PATH transfer. A remote Worker uses its own OS, account, configuration, and environment.
- Discover once per Worker tool-runtime lifetime and reuse the result across tool calls. Restarting the Worker refreshes discovery. Do not source shell profiles on each call or depend on a client restart. Use Codex as a reference for environment capture, not as a requirement to copy its probe verbatim.
- Select discovery by the Worker's OS and available trusted account/shell information. An interactive shell is a possible probe mechanism, not a universal Worker startup requirement; choose and verify supported shell modes on each OS. If a probe runs shell startup scripts, use the Worker account, a trusted working directory outside the workspace, a fixed capture command, closed stdin, no controlling TTY, bounded runtime/output, and cleanup of the probe process group on timeout. Accept only a validated PATH result. This is not a sandbox for startup scripts: capturing only PATH does not prevent side effects, and processes that deliberately leave the probe group are not contained.
- Supported account shells are zsh (interactive login, without a terminal) and Bash/sh/dash (noninteractive login). Bash does not read an interactive-only `.bashrc` with this probe; define execution PATH in the account's login profile or use `path_prepend`. Unsupported account shells use the configured and inherited entries.
- If discovery is unavailable, unsupported, malformed, or times out, record a typed, sanitized diagnostic and continue with configured and inherited PATH. Do not retry per tool call, mutate process-global PATH, or export discovered values to MCP/plugins, remote transports, logs, or model context.

## One effective PATH for Grep and Bash

```yaml
agent:
  kernel:
    tools:
      environment:
        path_prepend: []
```

1. Merge `agent.kernel.tools.environment.path_prepend` → discovered Worker PATH → inherited Worker PATH. Deduplicate exact directory entries, keeping the first occurrence. The executing Worker's operator owns `path_prepend`; it defaults to empty and takes effect on Worker restart. Do not accept client/workspace overrides.
2. Use native PATH split/join operations. Reject malformed, empty, or relative configured entries at config load, while allowing syntactically valid absolute directories that do not yet exist. Drop empty and relative discovered/inherited entries. The sandbox child environment is UTF-8, so drop unrepresentable discovered/inherited entries for both consumers. Do not expand shell expressions or guess install directories. Filter implicit directories inside the active workspace and project, including symlink aliases. Apply the same filter to inherited PATH before the startup probe for the Worker-configured workspace. Explicit operator configuration can still name a workspace directory.
3. Resolve executables in order, skipping broken and non-executable candidates. Grep uses the same list that Bash receives through the existing sandbox environment path. For zsh, Bash, sh and dash login invocations, restore the Worker PATH after startup files run and before evaluating the requested command. This preserves login startup and prevents system profiles from reordering configured entries. Other shells receive the initial PATH. Commands may intentionally modify their own PATH. An empty list must not trigger implicit working-directory lookup or shell-default lookup.
4. A supported native Worker adds its private `tool-bin` directory at the end of the list. Installed dependencies in configured, discovered and inherited directories keep precedence. The directory belongs to the Worker profile and is shared by both tools. An existing executable remains usable if the native installer or sandbox provider is unavailable. Only workspace-safe managed directories are added implicitly.

## Grep dependency recovery

| Condition on the executing Worker | Required behavior |
| --- | --- |
| Runnable `rg` is found on the effective PATH | Use it; do not invoke an installer. |
| `rg` is missing in an official Docker Worker image | Do not request approval or install at runtime. Check the effective PATH and advise repairing or replacing the image if its runtime dependency is missing. |
| `rg` is missing on a native Worker and a supported route exists | Request explicit, one-time user approval for installing ripgrep on that Worker. Run the fixed installer inside the existing OS sandbox, then resolve the verified executable and continue the original Grep request. |
| Approval is denied or unavailable | Do not invoke an installer. Tell the user to install ripgrep on the executing Worker, or to add its directory to `agent.kernel.tools.environment.path_prepend` if already installed. |
| No supported route, installation fails, or `rg` still cannot run | Fail the affected Grep request with the reason and an actionable next step. Do not block Worker startup or loop on installation. |

Installation permission is an explicit authorization at the executing Worker, not inferred from a writable directory or from calling Grep. Reuse existing policy and execution controls where applicable; do not accept package names or installer commands from the model or workspace. A sandbox denial is not a dependency miss. Grep stays eligible for parallel scheduling when its executable is available. A call already scheduled in parallel fails if the executable disappears before dispatch, and a subsequent call can request installation. Concurrent installation attempts on the same Worker share one result. Public errors distinguish missing dependency, denial, and recovery failure without disclosing raw probe/installer output or host PATH values.

The native route supports macOS arm64 and x86_64, GNU Linux arm64, and Linux x86_64. It requires an available native sandbox, executable system download and extraction utilities, and network policy that allows requesting access. Download a pinned upstream ripgrep release over HTTPS, verify its SHA-256 before extracting the single executable, run `rg --version`, and publish it atomically in the private Worker directory. The installer has write access to that directory and the sandbox's private scratch only. On Linux with network access enabled, a system resolver symlink outside `/etc` needs a read-only mount of its target file. Workspaces, account profiles, and package managers are outside the installation scope. An install directory overlapping the workspace or a protected root disables the route. A failed approved attempt is shared across callers until the Worker restarts. A manually supplied executable can still be used.

Installation uses the existing sandbox approval exchange with a scope bound to the Worker runtime instance, the original Grep call, and the pinned release and destination. The UI identifies the executing Worker and the download. If searching also requires its own approval, request it independently after successful installation. If the Worker is replaced or restarted while an installation approval is pending, the caller must start a fresh Grep request.

## Acceptance

- Debug first with an isolated instance and disposable profile. Establish the minimal-PATH baseline with real Grep and sandboxed Bash calls. Use Finder vs terminal to reproduce the reported macOS case, without changing or restarting the running instance. A pre-implementation PATH injection experiment can isolate the cause but is not proof of the production design.
- On macOS and Linux, verify the same Worker runtime path with controlled fixtures, including a standalone Worker with no Desktop client. Cover PATH precedence and deduplication, invalid entries, workspace exclusion, config upgrade, executable selection, and execution by both consumers. Confirm the Worker owns discovery and does not consume client PATH.
- From Bash, invoke the repository pre-push hook with a synthetic ref update and configured Python 3.11 or newer. Repeat with an old or unavailable Python and verify the actionable diagnostic. The hook runs locally with no push.
- Cover successful, noisy, hanging, malformed, and unsupported discovery, timeout cleanup, configured/inherited fallback, reuse within a Worker lifetime, and refresh on restart. Verify sandbox behavior and that unrelated tool environments remain unchanged.
- Exercise existing `rg`, authorized recovery and successful continuation, denied installation with no installer invocation, unsupported or failed installation, post-install resolution failure, concurrent misses, and sandbox denial. Use fixture installers; automated tests must not modify host packages. Re-run the packaged Finder case after implementation and distinguish it from hosted coverage.
