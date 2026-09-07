# Domain model and scope

This document owns the vocabulary shared by the [system](ARCHITECTURE.md),
[execution](RUNTIME_ARCHITECTURE.md) and [memory](MEMORY_ARCHITECTURE.md) views.
Names below describe existing boundaries, not proposed new services.

## Identity and ownership

```text
Trusted User + configured WorkspaceDefinition
                       |
                       v
       EffectiveWorkspace {workspace_id, user_id, root}
                       |
                       | scope for shared files and memory
                       v
          +-------------------------+
          | Product Session         |<---- exact Agent binding
          | durable ID/config/title |             |
          +------------+------------+      AgentDefinition
                       |                   identity/model/config
             contains logical Turns                |
                       |                           v
                       |                  AgentHarness / Provider
                       |                           |
                       +---------------------------+
                       v
              AgentContext for one Turn
              + optional Project / working directory
                       |
                       v
              Provider-owned AgentSession
              live handle + private resumable artifacts
```

Arrows indicate resolution or use, not filesystem nesting. An Agent does not own
a User's Workspace. Multiple Sessions using different Agents can share memory
when their trusted User and Workspace identities match.

| Object | Owner and meaning | Not equivalent to |
| --- | --- | --- |
| User | Authentication supplies the trusted identity | A client-provided directory segment |
| WorkspaceDefinition | Config owns logical ID and root prefix | Agent configuration or a Project |
| EffectiveWorkspace | Config resolves Workspace and User to an authorized root | Arbitrary client-supplied path |
| Project | Product-owned work target and directory association | The shared memory scope |
| Product Session | Service/Store own durable binding, configuration and lifecycle | Provider handle or prompt transcript |
| AgentDefinition | Configured Agent identity, Provider kind and typed configuration | An LLM provider alone |
| AgentSession | Provider-owned live execution/management handle | A second persisted Session record |
| Turn | One logical interaction identified by AgentTurnRef | Exactly one model call or invocation |
| AgentContext | Service-resolved input for one logical Turn | A serialized duplicate of Session state |
| DeploymentMode | Instance configuration-access boundary: local or cloud | Execution location, sandbox mode or Agent Provider |

## Workspace and working directory

```text
TALOS_HOME + WorkspaceDefinition.root_prefix + trusted user_id
                                |
                                v
                     EffectiveWorkspace.root
                     |-- MEMORY.md
                     `-- <memory_dirname>/...

Session + optional Project + current-directory/environment inputs
                                |
                                v
                     AgentContext.working_directory
```

Relative root prefixes resolve under Talos Home; configured absolute prefixes
remain absolute. Config validates IDs and overlapping prefixes. The default
definition is `default` with prefix `workspace-talos`. Other paths must be derived
through the existing configuration owner, not reconstructed from Agent names.
Working-directory resolution is a separate Service decision; changing the work
target does not itself create a new memory scope.

## Binding and lifecycle

- Resolve the initial Agent binding through product routing; dispatch later work
  with the Session's exact binding. Do not silently re-route each Turn.
- Service builds AgentSessionContext from durable binding and resolved Workspace,
  then AgentContext with Turn, model, permissions, Project and environment inputs.
  Pass these values through the execution layers; do not persist another snapshot.
- A logical Turn can release its live invocation while waiting for input and
  continue under the same identity. A terminal Turn is distinct from a waiting one.
- Provider `close` releases live resources without deleting resumable state;
  Provider `clear` clears private execution state while keeping the handle usable.
  Kernel clear deletes private Session artifacts. Reconstructing usable
  context does not guarantee recovery of the old native continuation. Product
  clear/history semantics belong to Service, not those SPI names.

## Source ownership

- [Workspace resolution](../../talos-rs/crates/infra/config/src/workspace.rs)
- [Session context construction](../../talos-rs/crates/orchestration/service/src/session/service.rs)
- [Execution context types](../../talos-rs/crates/orchestration/agent/src/context.rs)
- [Provider lifecycle contract](../../talos-rs/crates/orchestration/agent/src/provider.rs)
