# Web Context Compaction Event Presentation

Status: implemented
Owner: agent
Created: 2026-08-21
Updated: 2026-08-21
Related: `feat-040-web-light-theme-chat-presentation`, `refactor-039-memory-storage-retrieval`, `refactor-040-agent-runtime-message-event-api`, `archive/2026/refactor-037-web-desktop-timeline-render-runtime`

## 1. Conclusion

This change gives Web and Desktop one deterministic presentation rule for existing context-compaction Timeline events:

1. `user_requested` compaction renders only in a focused status surface above the Composer.
2. Turn-internal automatic compaction renders only as a process item in the current Turn Timeline.
3. `working_context_exceeded` remains an automatic Timeline event, but uses copy that describes active-loop trimming rather than session-history summarization.
4. The backend `trigger`, `stage`, `outcome`, item status, and metrics remain the sole facts. The frontend does not synthesize stages, percentages, step counts, or completion.
5. The same backend item identity stays on the same presentation surface for its entire lifecycle. A running item never moves between Composer and Timeline.
6. Manual compaction remains recoverable while running, but its completed historical maintenance event is not rendered as a chat message after reload.
7. The change is frontend-only. It does not modify `session.compact`, Timeline event schemas, Rust types, SDK types, TUI behavior, or backend persistence.

## 2. Background

Talos already emits typed context-compaction Timeline events with:

```text
trigger
stage
outcome
metrics
message
```

The backend stages are:

```text
preparing
summarizing
rebuilding_prompt
trimming
completed
failed
```

The current Web projection keeps only `stage` when converting protocol content into `ChatTimelineItem`. It drops `trigger`, `outcome`, and `metrics`. `useSessionCommands` then treats any active compaction stage as a manual command response. This creates two visible representations for automatic Turn compaction:

```text
Current Turn Timeline
└── Context compaction item

Composer area
└── Context compaction command card
```

The duplicate surfaces imply two operations even though the backend emitted one item. The frontend also cannot distinguish session-history compaction from working-context trimming, cannot render terminal outcomes accurately, and may display a generic mutation-pending state instead of the live backend stage for idle-session `/compact`.

## 3. Goals

1. Preserve the complete context-compaction event projection required for presentation.
2. Route each compaction item to exactly one visible surface based on `trigger`.
3. Render every backend stage without inventing progress.
4. Give manual, automatic history, and automatic working-context compaction distinct user-facing copy.
5. Preserve draft text and expose the existing Stop action while compaction is running.
6. Restore the correct running presentation after Timeline replay or reconnect.
7. Preserve existing Process disclosure behavior: active Turn work is expanded, and completed Turn work collapses normally.
8. Cover live events, terminal outcomes, cancellation, recovery, narrow layouts, reduced motion, and theme compatibility with deterministic tests.

## 4. Non-goals

1. No Rust, Service, Kernel, Store, Gateway, SDK, HTTP, SSE, app-server, or schema changes.
2. No new compaction stage, outcome, trigger, operation ID, event bus, progress API, or polling API.
3. No numeric progress percentage or fixed step indicator such as `1/3`.
4. No attempt to interpret `0` as a measured post-compaction token count for manual compaction.
5. No redesign of the compaction algorithm, thresholds, summary generation, fallback trimming, or context-usage calculation.
6. No duplicate permanent chat notice for manual `/compact`.
7. No rewrite of the general Timeline renderer, Process disclosure, Composer editor, or message queue.
8. No new animation, icon, state-management, or component library.

## 5. Existing Backend Contract

### 5.1 Trigger

| Trigger | Meaning | Presentation owner |
| --- | --- | --- |
| `user_requested` | User explicitly executed `/compact` | Composer status surface |
| `threshold_exceeded` | Prompt usage reached the configured compaction threshold | Turn Timeline |
| `hard_threshold_exceeded` | Prompt usage reached the hard threshold | Turn Timeline |
| `budget_exceeded` | Prompt exceeded its effective budget | Turn Timeline |
| `history_sections_dropped` | Session history no longer fit in the prompt | Turn Timeline |
| `working_context_exceeded` | Active model/tool-loop messages exceeded the working budget | Turn Timeline |

### 5.2 Stage

| Stage | Meaning | Terminal |
| --- | --- | --- |
| `preparing` | Resolve and prepare source context | No |
| `summarizing` | Generate or update compact session history | No |
| `rebuilding_prompt` | Rebuild the active prompt from compacted history | No |
| `trimming` | Remove older prompt or loop context to fit the budget | No |
| `completed` | Compaction maintenance finished | Yes |
| `failed` | Compaction maintenance failed | Yes |

`rebuilding_prompt` and `trimming` are conditional. The frontend must not assume that every operation visits every non-terminal stage.

### 5.3 Outcome

| Outcome | Presentation severity |
| --- | --- |
| `applied` | Success |
| `no_changes` | Neutral success |
| `rejected_missing_summary` | Warning |
| `trimmed_fallback_applied` | Warning success |
| `failed_context_still_too_large` | Error |
| `failed_provider_error` | Error |
| `failed_internal_error` | Error |

Cancellation remains authoritative through the Timeline item status `cancelled`. It must not be inferred from `failed` alone.

### 5.4 Metrics

The existing event can carry:

```text
covers_from_seq
covers_until_seq
pre_compaction_total_tokens
post_compaction_total_tokens
compaction_input_tokens
compaction_output_tokens
trimmed_context_unit_count
raw_tail_preferred_count
raw_tail_backfilled_count
working_context_input_messages
working_context_compacted_messages
working_context_preserved_recent_exchanges
```

Manual `/compact` does not rebuild the active prompt and currently leaves prompt before/after fields at their default zero values. This Spec therefore does not show manual before/after token reduction.

Automatic compaction may show a terminal token reduction only when all of the following are true:

```text
trigger != user_requested
pre_compaction_total_tokens > 0
post_compaction_total_tokens > 0
```

Metrics never drive lifecycle state. `stage`, `outcome`, and Timeline item status remain authoritative.

## 6. Frontend Projection

The frontend compaction item must retain the backend fields required by presentation:

```ts
type ChatContextCompactionItem = {
  itemId: string;
  sourceItemId?: string;
  kind: "context_compaction";
  phase: TalosTimelinePhase;
  status: TalosTimelineItemStatus;
  title?: string | null;
  text: string;
  trigger: ContextCompactionTrigger;
  stage: ContextCompactionStage;
  outcome?: ContextCompactionOutcome | null;
  metrics?: ContextCompactionMetrics | null;
  startedAtMs: number;
  completedAtMs?: number | null;
};
```

This is not a second protocol DTO. It is the existing UI projection extended with fields that are already consumed by presentation.

The active-state selector returns the full item rather than only the stage:

```ts
activeContextCompaction(stream): ChatContextCompactionItem | undefined
```

The selector must use stable backend item identity and item status. It must not combine independent items or derive state from mutation flags.

## 7. Presentation Routing

```text
Timeline ContextCompaction item
              |
              v
        inspect trigger
              |
       +------+------+
       |             |
       v             v
 user_requested   automatic trigger
       |             |
       v             v
 Composer card    Turn Timeline
       |             |
       +------X------+
          no duplicate
```

Routing rules:

1. `user_requested` is omitted from `MessageTimelineBlock` presentation.
2. Filtering occurs before Process item counts and disclosure visibility are derived, so a manual-maintenance-only block does not leave an empty `Working` or `Worked` row.
3. A live `user_requested` item drives the Composer compaction surface.
4. A terminal `user_requested` item updates the existing Composer surface in place.
5. Automatic items remain in their original Turn Timeline during live streaming, history loading, and recovery.
6. Automatic items never create `ComposerCommandResponse` state.
7. A mutation pending before the first event may show a short `Starting context compaction…` fallback, but the first backend item replaces it immediately.
8. A pending mutation must never override a newer backend terminal event.

## 8. Scenario A: Manual `/compact`

### 8.1 Running layout

```text
┌──────────────────────────────────────────────────────────────────────┐
│  ◌  Compacting context                                  Preparing   │
│     Preparing session context…                                      │
│     ─────── moving indeterminate signal ─────────────────────────    │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Continue with the migration plan once context is ready.              │
│                                                                      │
│  +   Full access                  Model · High    84%             [■] │
└──────────────────────────────────────────────────────────────────────┘
    Draft remains editable. Stop cancels the active compaction.
```

Interaction rules:

1. The status surface is directly above the Composer and aligned to its width.
2. The title remains `Compacting context` for all non-terminal stages.
3. Only the detail and subdued right-side stage label change.
4. The signal is indeterminate and communicates activity, not completion percentage.
5. The textarea remains editable and preserves selection, composition, pasted attachments, and draft text.
6. The normal Send action becomes the existing Stop action while the maintenance Turn is active.
7. Session commands, model changes, project changes, permission changes, and submission remain unavailable while the backend item is running.
8. The running surface has no close or Hide action. Dismissing visible progress while the operation continues would produce false state.

### 8.2 Stage copy

| Stage | Stable title | Detail | Stage label |
| --- | --- | --- | --- |
| `preparing` | `Compacting context` | `Preparing session context…` | `Preparing` |
| `summarizing` | `Compacting context` | `Summarizing earlier conversation while preserving recent work…` | `Summarizing` |
| `rebuilding_prompt` | `Compacting context` | `Rebuilding the prompt with compacted history…` | `Rebuilding` |
| `trimming` | `Compacting context` | `Trimming older context to fit the model limit…` | `Trimming` |

Manual compaction currently emits `preparing`, `summarizing`, and a terminal stage. The renderer still supports all protocol stages so it remains correct if backend execution later emits another valid stage.

### 8.3 Completed layout

```text
┌──────────────────────────────────────────────────────────────────────┐
│  ✓  Context compacted                                               │
│     Ready to continue with refreshed context.                       │
└──────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────┐
│ Continue with the migration plan once context is ready.              │
│                                                                      │
│  +   Full access                  Model · High    updated         [↑] │
└──────────────────────────────────────────────────────────────────────┘
```

The terminal success surface remains visible for four seconds, unless replaced by another user action. It then dismisses without moving content into Timeline.

### 8.4 Failed and cancelled layouts

```text
┌──────────────────────────────────────────────────────────────────────┐
│  !  Couldn’t compact context                                    [×] │
│     The model provider was unavailable. Try again shortly.           │
│                                                   [Try again]        │
└──────────────────────────────────────────────────────────────────────┘
```

```text
┌──────────────────────────────────────────────────────────────────────┐
│  –  Compaction stopped                                          [×] │
│     Session context was not changed.                                │
└──────────────────────────────────────────────────────────────────────┘
```

Error and cancelled surfaces persist until dismissed or replaced. Retry executes the existing `/compact` action; it does not create a separate retry protocol.

### 8.5 Recovery behavior

```text
reload / reconnect
        |
        v
recover Timeline item
        |
        +-- user_requested + running  --> restore Composer card + Stop
        |
        +-- user_requested + terminal --> no historical chat item
```

The Composer surface is reconstructed from recovered backend state, not React mutation state. Completed maintenance events remain persisted for audit and recovery but do not reappear as old Composer notices after a normal reload.

## 9. Scenario B: Automatic Session-History Compaction

The automatic title uses grammatically natural English: `Automatically compacting context`. The previous phrase `Context automatically compacting` is not used.

### 9.1 Running Timeline

```text
Working for 8.4s                                                   ^
────────────────────────────────────────────────────────────────────

  ◌──  Automatically compacting context                 Summarizing
  │    Summarizing earlier conversation to free context…
  │
  ◌    Thinking
       Waiting for refreshed context…
```

The context item is part of the existing Process rail. It is not a chat bubble, detached notice, or Composer card.

### 9.2 Stage copy

| Stage | Title | Detail |
| --- | --- | --- |
| `preparing` | `Automatically compacting context` | `Preparing session history before continuing the turn…` |
| `summarizing` | `Automatically compacting context` | `Summarizing earlier conversation to free context…` |
| `rebuilding_prompt` | `Automatically compacting context` | `Rebuilding the prompt with compacted history…` |
| `trimming` | `Automatically compacting context` | `Trimming older context to fit the model limit…` |

Trigger-specific reasons may be exposed through accessible text or a low-priority detail, but raw enum names are not user-visible:

| Trigger | Optional reason copy |
| --- | --- |
| `threshold_exceeded` | `Context usage reached the compaction threshold.` |
| `hard_threshold_exceeded` | `Context is approaching the model limit.` |
| `budget_exceeded` | `The current prompt exceeded its context budget.` |
| `history_sections_dropped` | `Some earlier history no longer fits in the prompt.` |

### 9.3 Completion and Turn continuation

```text
Working for 12.1s                                                  ^
────────────────────────────────────────────────────────────────────

  ✓──  Context compacted
  │    Continuing the turn with refreshed context.       128K → 74K
  │
  ◌──  Thinking
  │    Preparing the next response…
  │
  ◆    Final answer continues in the normal response area
```

The compaction item becomes terminal in place. The Turn continues with subsequent reasoning, tool, commentary, or answer items. The Process disclosure collapses only when the Turn itself completes, following existing behavior.

The optional token transition renders only when the existing metrics satisfy the validity rules in section 5.4.

## 10. Scenario C: Automatic Working-Context Trimming

Working-context maintenance affects active model/tool-loop messages, not stored session-history summarization. It therefore has different copy.

### 10.1 Running Timeline

```text
Working for 46.8s                                                  ^
────────────────────────────────────────────────────────────────────

  ◌──  Reducing working context                         Trimming
  │    Trimming older tool and reasoning history while preserving
  │    recent work…
  │
  ◌    Turn paused before the next model request
```

### 10.2 Completion

```text
  ✓──  Working context reduced
  │    Continuing the turn with recent work preserved.    96K → 68K
  │
  ◌──  Thinking
       Preparing the next response…
```

### 10.3 Failure

```text
  !──  Working context is still too large                  Failed
       The turn cannot continue within the model’s context limit.
```

The copy must not claim that session history was summarized when the trigger is `working_context_exceeded`.

## 11. Terminal Outcome Copy

### 11.1 Manual Composer copy

| Outcome or status | Title | Detail |
| --- | --- | --- |
| `applied` | `Context compacted` | `Ready to continue with refreshed context.` |
| `no_changes` | `Context already compact` | `No new session history needed to be compacted.` |
| `rejected_missing_summary` | `Context unchanged` | `A usable session summary could not be created.` |
| `trimmed_fallback_applied` | `Context trimmed` | `Older context was trimmed to fit the model limit.` |
| `failed_context_still_too_large` | `Context is still too large` | `Compaction could not bring the context within the model limit.` |
| `failed_provider_error` | `Couldn’t compact context` | `The model provider was unavailable. Try again shortly.` |
| `failed_internal_error` | `Couldn’t compact context` | `An internal error interrupted context compaction.` |
| item status `cancelled` | `Compaction stopped` | `Session context was not changed.` |

### 11.2 Automatic Timeline copy

| Outcome or status | Title | Detail |
| --- | --- | --- |
| `applied` | `Context compacted` | `Continuing the turn with refreshed context.` |
| `no_changes` | `Context checked` | `No additional compaction was needed; continuing the turn.` |
| `rejected_missing_summary` | `Context summary unavailable` | `Continuing with the available context.` |
| `trimmed_fallback_applied` | `Context trimmed` | `Older context was trimmed; continuing the turn.` |
| `failed_context_still_too_large` | `Context is still too large` | `The turn cannot continue within the model’s context limit.` |
| `failed_provider_error` | `Context compaction failed` | `The model provider was unavailable.` |
| `failed_internal_error` | `Context compaction failed` | `An internal error interrupted the turn.` |
| item status `cancelled` | `Compaction stopped` | `The active turn was stopped.` |

Outcome copy is selected from typed enums. Backend `message` is a diagnostic fallback and must not become the primary localization key.

## 12. Visual and Motion Rules

1. Consume existing Talos theme tokens for card, surface, border, ink, muted text, brand, success, danger, and shadow.
2. Manual running status uses the same width, radius family, border weight, and shadow hierarchy as existing Composer response surfaces.
3. Automatic status uses the existing Timeline rail and item spacing. It does not introduce a nested card.
4. Running motion is limited to one subtle spinner or orbit plus one indeterminate one-pixel signal on the manual surface.
5. No large-area shimmer, pulsing card, gradient glow, percentage animation, or bouncing text.
6. Stage updates cross-fade only the detail and stage label. Container height and title baseline remain stable.
7. `prefers-reduced-motion` removes continuous motion and keeps a static running indicator.
8. Success uses low-saturation semantic color. Failure uses the existing danger color without filling the whole surface.
9. Narrow layouts hide the secondary stage label before truncating the primary detail.

## 13. Interaction Boundaries

### 13.1 Composer and suggestions

- A running manual compaction surface has priority over command suggestions and model pickers.
- `/` and `$` suggestions do not open while the Session has a running compaction item.
- The editor remains editable; suggestion suppression must not alter cursor position or draft text.
- Running automatic compaction does not create a Composer card. Existing active-Turn controls continue to own Stop and busy behavior.

### 13.2 Stop, dismiss, and retry

- Running items cannot be dismissed.
- Stop uses the existing active-Turn cancellation path.
- Manual success auto-dismisses after four seconds.
- Manual warning, failure, and cancellation remain until dismissed or replaced.
- Automatic Timeline items are never manually dismissed; they follow Process disclosure behavior.
- Retry is offered only for manual provider/internal failures and calls the existing compact action.

### 13.3 Context usage indicator

- The context usage ring remains mounted during compaction.
- Manual completion refreshes Session data through the existing mutation success path.
- The ring does not derive its value from compaction event metrics.
- A new Session with no measured context continues to hide the ring.
- Compaction must not replace a valid measured context snapshot with zero or null presentation state.

## 14. Subscription and Recovery

The current subscription condition is centered on ordinary streamed Turn state. Manual `/compact` can begin while the Session is otherwise idle, so the frontend must keep or start Timeline observation for the maintenance Turn.

```text
User selects /compact
        |
        v
enable Session Timeline observation from last stable cursor
        |
        v
start existing session.compact mutation
        |
        +-- item_started / delta --> Composer card
        |
        +-- terminal event --------> terminal Composer state
        |
        +-- HTTP success ----------> refresh Session query
```

Requirements:

1. Event subscription is established before or atomically with starting the mutation.
2. Recovery from the last stable cursor closes any race between mutation start and live subscription.
3. Mutation pending is only a startup fallback.
4. A recovered running `user_requested` item restores busy and Stop state even when no local mutation exists.
5. A recovered automatic item remains in Timeline and never becomes a manual card.
6. A terminal event clears the active compaction guard independently of text streaming.
7. Turn terminal events close stale running presentation if an expected item terminal event is unavailable.
8. Observation and ephemeral command state belong to one Session. Changing routes stops observing the previous Session and cannot move its running or terminal state into the destination Session.
9. HTTP success refreshes Session data but is not a compaction lifecycle terminal. The matching backend item remains authoritative for outcome presentation.

## 15. Component Boundaries

Use existing feature directories and split only by real presentation ownership:

```text
features/chat/
├── stream-events.ts
├── timeline-state.ts
├── hooks/
│   ├── useChatSession.ts
│   └── useChatSessionTimelineLifecycle.ts
├── components/
│   ├── command-response/
│   │   ├── ComposerCommandResponse.tsx
│   │   └── ComposerCompactionResponse.tsx
│   └── ...
pages/chat/
├── useSessionCommands.ts
└── useChatSessionComposer.ts
widgets/message-list/
├── MessageTimelineBlock.tsx
└── context-compaction/
    └── ContextCompactionTimelineItem.tsx
```

Responsibilities:

| Owner | Responsibility |
| --- | --- |
| `stream-events.ts` | Preserve typed protocol fields in the existing UI projection |
| `timeline-state.ts` | Select the active full compaction item and expose pure presentation predicates |
| `useChatSessionTimelineLifecycle.ts` | Scope Timeline recovery, idle maintenance observation, and compaction selection to the current Session |
| `useSessionCommands.ts` | Start manual compaction and own only ephemeral manual terminal response timing |
| `ComposerCompactionResponse.tsx` | Render manual running and terminal states above Composer |
| `ContextCompactionTimelineItem.tsx` | Render automatic history and working-context items inside existing Timeline |
| `MessageTimelineBlock.tsx` | Route Timeline item rendering without owning compaction copy or state transitions |

Do not introduce a generic command framework, operation registry, second event reducer, context-compaction store, or parallel state machine.

## 16. State and Rendering Invariants

1. One backend compaction item produces at most one visible compaction surface.
2. Surface ownership is a pure function of `trigger`.
3. Lifecycle is a pure function of item status, stage, and outcome.
4. Mutation state cannot override backend terminal state.
5. Item updates preserve `itemId`, React key, animation container, and surface location.
6. No valid stage produces an empty title or detail.
7. Manual operation state is matched by both `sessionId` and backend `itemId`; neither HTTP completion nor a previous terminal item may settle a new operation.
7. Unknown future enum values fail visibly through the existing protocol/type boundary; they are not silently mapped to success.
8. Missing metrics hide metric copy without changing lifecycle copy.
9. Zero manual before/after values never render as `0 → 0`.
10. Manual historical events remain recoverable but do not render as permanent chat content.

## 17. Accessibility

1. Manual surface uses `role="status"` and `aria-live="polite"` while running.
2. Failed manual surface uses `role="alert"` only when the terminal failure arrives.
3. Stage changes announce one concise detail; decorative stage labels and motion are hidden from assistive technology.
4. Stop, Dismiss, and Retry have explicit accessible names.
5. Color is not the only success or failure signal; icons and text remain present.
6. Focus remains in the Composer when stage deltas arrive.
7. Auto-dismiss does not move focus.
8. Reduced-motion mode presents the same semantic state without continuous animation.

## 18. Test Strategy

### 18.1 Projection tests

- Preserve `trigger`, `stage`, `outcome`, and metrics from live `item_started` and `item_delta` events.
- Preserve the same fields from history and recovery snapshots.
- Update one item in place across all valid stage transitions.
- Clear active state on `completed`, `failed`, `cancelled`, and Turn terminal recovery.

### 18.2 Routing tests

- `user_requested` running item renders in Composer and not Timeline.
- Every automatic trigger renders in Timeline and not Composer.
- `working_context_exceeded` selects working-context copy.
- A terminal item remains on its original surface.
- Multiple sequential compaction items retain separate backend identities.

### 18.3 Composer behavior tests

- Manual stage deltas update detail and stage label without clearing draft.
- Stop remains available while running.
- Send and commands recover after terminal completion.
- Success auto-dismisses after four seconds.
- Error and cancellation remain dismissible.
- Reconnect restores a running manual card without local mutation state.
- Context usage indicator stays mounted when a valid measured snapshot exists.

### 18.4 Timeline behavior tests

- Automatic history compaction renders all supported stages.
- Optional stages may be skipped without synthetic placeholders.
- Completed compaction is followed by later Turn items in server order.
- Working-context trimming never claims that session history was summarized.
- Process disclosure remains open while the Turn runs and follows existing completion collapse behavior.

### 18.5 Visual tests

- Snapshot manual `preparing`, `summarizing`, success, error, and cancellation.
- Snapshot automatic `summarizing`, `rebuilding_prompt`, `trimming`, completion, and failure.
- Snapshot working-context trimming and completion.
- Verify light theme, dark theme, narrow width, long localized copy, and reduced motion.
- Inspect every snapshot diff; do not accept generated snapshots without visual review.

### 18.6 Real user-path verification

1. Start frontend and backend with the development YAML configuration.
2. Execute `/compact` from an idle Session.
3. Verify live Composer stages, preserved draft, Stop, terminal result, query refresh, and no Timeline duplicate.
4. Trigger threshold compaction during a real Turn.
5. Verify Timeline-only presentation, Turn continuation, later model output, and no Composer card.
6. Trigger working-context trimming with a bounded test configuration.
7. Verify working-context copy, metrics visibility rules, and continuation or explicit failure.
8. Reload during running manual and automatic compaction and verify recovered presentation.

## 19. Compatibility and Breaking-Change Review

- Backend protocol: unchanged.
- `session.compact` request and response: unchanged.
- Timeline storage and replay: unchanged.
- SDK public types: unchanged.
- TUI presentation: unchanged.
- Web internal `ChatTimelineItem` projection: extended with existing protocol fields.
- Desktop behavior: updated through the shared Web bundle.
- Historical `user_requested` events remain persisted but are intentionally omitted from chat presentation.

## 20. Acceptance Criteria

1. Manual and automatic compaction never render simultaneously in Composer and Timeline.
2. All six existing backend stages have explicit presentation copy.
3. All existing outcomes and cancellation have explicit terminal copy.
4. The frontend renders no fabricated progress percentage, fixed step count, or inferred completion.
5. Running manual compaction preserves draft and exposes Stop.
6. Automatic compaction remains part of the active Turn Timeline and the Turn visibly continues afterward.
7. Working-context trimming uses distinct, accurate copy.
8. Reload and reconnect reconstruct the same surface from Timeline state.
9. Manual `0 → 0` token metrics are never displayed.
10. Existing context usage remains visible unless the Session has no measured context.
11. Frontend unit, component, snapshot, typecheck, lint, and real user-path verification pass.
12. No backend, API, SDK, schema, TUI, or unrelated UI files change as part of this implementation.
