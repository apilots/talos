# Memory architecture

Memory is scoped by trusted User and Workspace, independently of Agent identity.
See [Domain model](DOMAIN_MODEL.md) for resolution and working-directory rules.

## Sources and projections

```text
               User + Workspace scope
                         |
          +--------------+-------------------+
          |                                  |
          v                                  v
   MEMORY.md source                  Dream Memory sources
   stable keyed guidance             <memory_dirname>/YYYY-MM-DD.md
          |                                  |
          v                           +------+----------------+
   validated snapshot                 |                       |
          |                           v                       v
          |                    derived OKF bundle      searchable DB index
          |                           |                       |
          v                           +-----------+-----------+
   bounded prompt injection                       |
                                                  v
                                         bounded retrieval

   Product event DB                     Provider private artifacts
   execution facts + notices            native context / continuation
   [durable product truth]              [execution implementation state]
```

These are distinct sources, not a MEMORY.md-to-Dream-to-SQLite conversion pipeline.
Dream search projections can be rebuilt from validated source documents. This
does not make the product event database disposable or Provider artifacts a
replacement for public History. Backups must account for each owned state type.
Do not assume private continuation artifacts are reconstructable from public
History. Preserve them when retaining native resume capability; Provider `clear`
is an explicit destructive lifecycle operation, not an index-rebuild procedure.

## Responsibilities

| Boundary | Responsibility |
| --- | --- |
| talos-knowledge-format | Shared structured document format |
| talos-memory | Workspace/Dream document models, validation and capability ports |
| UserWorkspaceMemoryService | Typed entry updates, whole-document editing and snapshots |
| UserWorkspaceMemoryReflection | Bounded product policy using the Inner Model |
| MemoryService | Product access, trusted scope, editing and retrieval |
| MemoryStore | Dream source persistence, derived bundle/index consistency and queries |
| Prompt/Provider integration | Bounded model injection; no direct product-state mutation |

`MEMORY.md` records stable preferences, constraints, decisions and durable active
context. It is not a transcript or a place for routine tool output. Dream Memory
records dated experiences with structured provenance; generated entries retain
their source Session/range, while user-authored entries have explicit provenance.

## Writes and partial failure

```text
Whole MEMORY.md edit              Dream source replacement
         |                                 |
lock + expected version          scope lock + recover stale index
         |                                 |
validate document                compare expected content + validate
         |                                 |
atomic file replacement          mark derived index stale
         |                                 |
validated snapshot               write source -> sync bundle -> sync DB
                                           |
                                 clear stale marker on success
```

Typed entry mutations lock and read the latest document before applying their
operation. Whole-file edits use the expected version to reject lost updates.
Invalid existing memory is surfaced, not silently replaced. File writes reuse
the filesystem owner's atomic-write primitive.

A Dream index failure can occur after the source changed. The stale marker and
validated sources drive recovery; this is not a cross-filesystem/SQLite atomic
transaction. Do not delete the source to make an index failure disappear.

## Reflection and context limits

The prompt keeps durable mutation policy in its static prefix, memory data in a
bounded dynamic region, and operation details in the tool description. Memory
is relevant background, not authority over root/system rules, explicit current
instructions or newly verified facts. Task-local exceptions do not automatically
change durable preferences. Both explicit-tool guidance and reflection favor
minimal stable-key changes, preserve unrelated entries, and require an explicit
forget request or verified obsolescence before deletion.

Missing memory does not justify initialization on its own. Invalid content is not
trusted guidance or an invitation to repair; whole-document repair requires
explicit authorization and an observed version. Prompt guidance requires tool
success before claiming a change and forbids blind conflict retries or bypassing
the writer with direct filesystem writes. These are model instructions, not a
new authorization mechanism; validation, locking and version checks remain owned
by the existing Tool/Service boundaries.

Eligible successful primary-Agent Turns can trigger one bounded reflection
before terminal commit/publication. A successful explicit memory-tool update
suppresses redundant reflection. The Inner Model may choose no change, upsert a
stable entry or remove one; this is not a general self-modifying learning system.
Reflection failure is reported as a product warning without replacing the main
Turn result. Durable warning delivery still depends on Store availability.

The memory format and prompt injection enforce `MEMORY_FILE_MAX_CHARS`; reflection
also bounds supplied user/assistant text and has a timeout. Character limits are
not exact token counts. Context contributors must document source, clipping and
token assumptions, meet the 10K-token item ceiling, and flag new items above 1K
tokens for review. Explicit compaction does not authorize rewriting product facts.

## Source ownership

- [Memory updates](../../talos-rs/crates/orchestration/service/src/user_workspace_memory/service.rs)
- [Reflection policy](../../talos-rs/crates/orchestration/service/src/user_workspace_memory/reflection.rs)
- [Dream consistency](../../talos-rs/crates/orchestration/store/src/memory/consistency.rs)
- [Memory prompt injection](../../talos-rs/crates/common/prompt/src/memory.rs)
