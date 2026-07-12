# Web/Desktop Timeline Presentation and Tool Card Refinement

Status: draft
Owner: agent
Created: 2026-07-10
Updated: 2026-07-10

## Background

Talos Web already has the required framework and message pipeline. React renders the UI, Zustand owns live session state, React Query loads session history, Browser receives timeline events through HTTP/SSE, and Desktop adapts app-server events while loading the same Web bundle.

The current issues are limited to frontend state projection and presentation:

1. `MessageList.tsx::timelineTextSegments()` splits text at punctuation and turns complete sentences into synthetic lines.
2. `timeline-state.ts::commitTimelineAssistantTextInStream()` removes streamed text from its timeline item and creates a new assistant block when an `Unknown` item becomes `FinalAnswer`.
3. Reasoning and assistant commentary can both be labeled `Thinking` even though they have different roles.
4. A timeline tool item owns an outer header while `ToolCallBlock` owns another card header, producing duplicate tool chrome.
5. Typed tool result rendering already exists, but the tool title, status, summary, preview, and details need a clearer hierarchy.

These are `talos-app/web` concerns. Desktop receives the same fixes through the shared Web renderer.

## Existing Capabilities

The implementation continues to use the existing capabilities without changing their ownership:

- `useSessionHistoryQuery()` and `buildBlocksFromHistorySnapshot()` for history;
- `sendTimelineEvents()` and `subscribeTimelineEventsForMessage()` for incremental events;
- `applyStreamEvent()` and the current Zustand stream state;
- active turn, recovery, cancel, retry, pending input, and history invalidation behavior;
- Browser and Desktop transport adapters;
- React, Zustand, React Query, Testing Library, and Vitest;
- Desktop loading the shared `talos-app/web` bundle.

## Design Principles

1. Limit changes to Web presentation and the existing frontend phase transition.
2. Use the existing timeline `item_id` as stable identity across text deltas and phase changes.
3. Render model text according to model-provided line breaks.
4. Continue rendering real SSE or app-server deltas immediately.
5. Treat phase commit as an in-place role update for the same text item.
6. Keep the working/final response area separate from collapsible thinking and tool work.
7. Give each tool item one primary card owner.
8. Use TUI only as an information-hierarchy reference for tool cards.
9. Keep Browser and Desktop on the same Web implementation.

## Goals

1. Preserve complete thinking and commentary sentences in one text container.
2. Keep assistant text incremental from the first delta through phase commit.
3. Keep `Unknown -> FinalAnswer` content on the same timeline item and React identity.
4. Give reasoning, working text, commentary, execution summary, and final answer accurate labels and styles.
5. Present tool calls with a compact semantic title, state, typed summary, preview, and details.
6. Preserve existing history, live streaming, recovery, cancel, retry, and auto-scroll behavior.
7. Implement the refinement within current frontend component and store boundaries.

## Non-goals

1. Rust application service, Agent, Kernel, TUI, and timeline protocol changes.
2. TypeScript SDK, HTTP/SSE, app-server API, or schema changes.
3. History loading, recovery, or subscription rewrites.
4. A new timeline runtime, transcript store, event framework, cursor manager, or scheduler.
5. Shared TUI/Web rendering code, fixtures, styles, or interaction behavior.
6. A generic dynamic-input form, virtualized message list, or new rendering dependency.
7. Changes to any other active or archived specification.

## Existing Data Flow

```text
Browser SSE / Desktop app-server
              |
              v
      existing applyStreamEvent()
              |
              v
     existing Zustand stream state
              |
        +-----+-----+
        |           |
 history blocks   live blocks/items
        |           |
        +-----+-----+
              |
              v
 lightweight presentation helpers
              |
              v
       existing React message UI
```

The change is limited to the existing stream phase transition and the final presentation layer.

## Stable Item Identity

The protocol already provides `session_id`, `message_id`, and `item_id`. A timeline block ID scopes item identity in React:

```ts
function timelineItemKey(timelineId: string, itemId: string) {
  return `${timelineId}:${itemId}`;
}
```

Constraints:

1. React keys derive from the scoped timeline item identity.
2. `item_delta` appends to the existing item or its existing ordered source segments.
3. `assistant_text_committed` updates item phase in place.
4. Tool arguments, progress, result, and status continue updating the same tool item.
5. Local copy and expand state remains attached to stable rendered components.

## Timeline Text Presentation

### Faithful Text Layout

Timeline thinking, commentary, and execution summary text is rendered with `whitespace-pre-wrap`:

- model-provided `\n` and `\n\n` remain visible;
- ordinary punctuation does not create a line or list item;
- ordinary sentences do not receive synthetic bullets;
- copy continues to use the original text;
- final-answer content continues to use the existing Markdown renderer.

For example, `First thought. Second thought.` remains one text node when the model did not include a line break.

### Phase Continuity

`AssistantText/Unknown` follows this lifecycle:

1. The first text delta creates visible working text immediately.
2. Later deltas append to the same source item.
3. `AssistantText/Unknown` renders in a response area outside the collapsible process list.
4. A commentary commit updates the item role to `Commentary` for process presentation.
5. A final-answer commit updates the same response item label and style to `Final answer`.
6. The final-answer commit preserves item text, response DOM placement, and scoped React key.
7. Final-answer Markdown is rendered from the text already accumulated by real deltas.

`commitTimelineAssistantTextInStream()` therefore only projects the committed phase onto matching text items. It does not own answer-block creation.

### Copy Scope

Process items and response items expose separate copy actions:

- the process action copies only reasoning, commentary, execution summaries, and tool details;
- each working or final-answer response action copies only that item's original text;
- a phase commit keeps the response copy action attached to the same scoped item identity.

## Presentation Roles

| Timeline kind or phase | Presentation | Meaning |
| --- | --- | --- |
| `Reasoning/Thinking` | Thinking | Model reasoning with faithful paragraph layout. |
| `AssistantText/Unknown` | Working | Neutral provisional assistant text. |
| `AssistantText/Commentary` | Commentary | Short execution commentary. |
| `ToolCall/Acting` | Tool card | A single compact tool card. |
| `ExecutionSummary/System` | Execution summary | A compact summary distinct from reasoning. |
| `AssistantText/FinalAnswer` | Final answer | Markdown rendered from the accumulated item text. |
| `ContextCompaction/System` | Context status | Existing context status content. |
| `SystemNotice/System` | System | Existing notice title and severity content. |

Labels derive only from item kind and phase.

Reasoning, commentary, execution summary, and tool cards remain in the alternating process list. `AssistantText/Unknown` and `AssistantText/FinalAnswer` share the response area, so a final commit changes presentation without moving accumulated text between UI regions.

## Tool Card Presentation

### TUI Reference Scope

The Web card adopts the useful information order from the TUI command cards:

- a semantic title based on the tool's primary argument;
- compact running, completed, waiting, and failed state;
- one or two preview lines from typed result content;
- full result, input, progress, prompt, and call ID in expandable details.

The implementation remains a normal React component using existing Web state and styles.

### Single Card Owner

Each timeline tool item renders one primary card:

```text
icon + semantic title + status + duration
compact typed preview
expandable body
  typed result
  dynamic prompt
  details
    call id
    arguments
    progress
```

`ToolCallBlock` and `ToolCallCard` own this card. The generic timeline text header wraps text items only.

The header projects status directly from `ChatToolStatus`. Duration is a separate field and appears once, using item timing with typed Bash elapsed time as a fallback. Typed result summary and preview remain below the header. Long semantic titles shrink and truncate before the status, duration, or disclosure control.

### Semantic Tool Title

The message-list presentation layer contains a small pure helper that projects a title from the existing argument text. It does not enter Zustand and does not duplicate tool result state.

Primary argument priority:

| Tool family | Preferred argument |
| --- | --- |
| Bash / Shell / Exec | `cmd`, `command` |
| Read / Write / Edit / Patch | `file_path`, `path`, `file` |
| Grep / Search | `pattern`, `query` |
| Glob | `pattern`, `glob` |
| Web / Reference | `url`, `domain`, `query` |
| Generic | first non-empty string argument |

The title preview has a hard character cap. The complete argument value remains in details. Argument text that is not yet a complete structured value keeps the base tool name until a valid projection is available.

### Typed Result Summary

The existing `buildToolResultModel()` remains the source of tool result summary, preview, and body:

- `bash_output` projects execution state, exit code, and the first output line;
- `diff` projects file and line counts;
- `file_read` projects path and line coverage;
- `search_results` projects match count and representative matches;
- Web, artifact, and MCP references project their reference labels;
- text, JSON, progress, and fallback output use the existing generic projection.

No second tool content model is stored.

## History and Live Rendering

History and live data lifecycles remain unchanged:

- history snapshots continue through the existing query and builder;
- live items continue through the existing stream store;
- both use the same timeline labels, text presentation, tool title projection, and tool result projection at render time;
- the existing live-to-history handoff remains responsible for replacing completed live blocks with canonical history.

## Frontend File Boundary

Implementation changes are limited to:

- `talos-app/web/src/features/chat/timeline-state.ts`
- `talos-app/web/src/features/chat/__tests__/streaming.test.ts`
- `talos-app/web/src/widgets/message-list/MessageList.tsx`
- `talos-app/web/src/widgets/message-list/MessageList.test.tsx`
- `talos-app/web/src/widgets/message-list/ToolCallBlock.tsx`
- `talos-app/web/src/widgets/message-list/ToolCallBlock.test.tsx`
- `talos-app/web/src/widgets/message-list/ToolResultRenderer.tsx`
- `talos-app/web/src/widgets/message-list/ToolResultRenderer.test.tsx`
- `talos-app/web/src/widgets/message-list/toolDisplay.ts`
- `talos-app/web/src/widgets/message-list/toolDisplay.test.ts`
- `talos-app/web/src/widgets/tool-call-card/ToolCallCard.tsx`
- `talos-app/web/src/shared/i18n/resources.ts`

## Incremental Update Rules

1. Existing Zustand updates remain event-driven for every real delta.
2. Text deltas update the matching timeline item content.
3. Phase and status events update matching item properties.
4. The frontend does not replay completed text with timers or character animation.
5. Auto-scroll continues through `useChatSessionAutoScroll()`.

## Test Coverage

### Text and Phase

1. A complete punctuated sentence remains in one faithful text container with original line breaks.
2. `Unknown -> deltas -> ItemCompleted -> Committed(FinalAnswer)` preserves the item text and phase on the same state object identity.
3. The scoped timeline item DOM element remains stable across `Working -> Final answer`.
4. Final-answer content renders through Markdown after the phase commit.
5. Commentary renders with the `Commentary` label.
6. The response copy action writes the original response text for the stable item.

### Tool Card

1. Bash, file, search, and Web tools produce semantic titles from representative arguments.
2. Long title previews use the configured hard cap.
3. A timeline tool item renders one primary tool card.
4. Waiting, completed, and failed states render from the actual tool status.
5. Typed Bash and failed Diff results produce their expected summaries and previews.
6. Status, one duration, result summary, and preview occupy separate presentation fields.
7. Expanded details expose call ID, arguments, progress, prompt, and typed result content.

### Verification Boundary

1. Scoped Vitest state, pure helper, and component tests cover the changed behavior.
2. Web typecheck and lint validate frontend boundaries and types.
3. Integration tests and browser E2E are outside this implementation run.

## Breaking Change Review

There is no public API, schema, SDK, Rust, Desktop transport, or TUI breaking change.

Internal Web behavior changes are limited to:

- committed unknown assistant text remains on its timeline item;
- timeline prose follows original line breaks;
- commentary and final answer have explicit roles;
- timeline tools have one primary card owner;
- tool titles include a bounded semantic argument preview.

## Completion Criteria

1. Timeline prose preserves complete sentences and original line breaks.
2. Assistant text remains incremental through phase commit.
3. Final commit preserves accumulated text and scoped item identity.
4. Thinking, working, commentary, execution summary, and final answer are distinct.
5. Tool calls use a single compact card with typed summary and details.
6. Browser and Desktop continue sharing the Web implementation.
7. Existing history and live lifecycle ownership remains unchanged.
8. The diff contains no runtime, protocol, SDK, Rust, TUI, archived Spec, or unrelated documentation changes.
9. Scoped unit/component tests, Web typecheck, and Web lint pass.
