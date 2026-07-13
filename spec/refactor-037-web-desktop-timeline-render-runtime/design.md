# Web/Desktop Timeline Presentation and Tool Step Refinement

Status: implementation
Owner: agent
Created: 2026-07-10
Updated: 2026-07-13

## Background

Talos Web already has the required framework and message pipeline. React renders the UI, Zustand owns live session state, React Query loads session history, Browser receives timeline events through HTTP/SSE, and Desktop adapts app-server events while loading the same Web bundle.

The current issues are limited to frontend state projection and presentation:

1. `MessageList.tsx::timelineTextSegments()` splits text at punctuation and turns complete sentences into synthetic lines.
2. The server-provided timeline phase must be the sole authority for placing assistant text in the process or final-answer region.
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
5. Treat the server-provided phase and phase commit as the authoritative presentation state.
6. Keep the final response area separate from collapsible process work.
7. Give each tool item one primary card owner.
8. Use TUI only as an information-hierarchy reference for tool steps.
9. Keep Browser and Desktop on the same Web implementation.

## Goals

1. Preserve complete thinking and commentary sentences in one text container.
2. Keep explicitly phased final-answer text incremental from its first delta.
3. Keep `Unknown -> FinalAnswer` content on the same timeline state item across phase commit.
4. Give reasoning, commentary, execution summary, and final answer accurate labels and styles.
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
6. Explicit final-answer starts, unknown-to-final commits, and history snapshots all project through the same timeline block and server item ID.

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
3. `AssistantText/Unknown` renders in the process timeline with Commentary presentation.
4. A commentary commit preserves that process presentation.
5. A final-answer commit updates the same stored item to `FinalAnswer` and moves its accumulated text into the final-answer region.
6. The phase commit preserves the source item ID and accumulated text.

When `item_started` already carries `FinalAnswer`, the final-answer region renders from the first delta and continues incrementally. The frontend does not infer a final answer from item order or from an `Unknown` item being the latest text item.

`commitTimelineAssistantTextInStream()` therefore only projects the committed phase onto matching text items. It does not own answer-block creation.

### Copy Scope

Process items and response items expose separate copy actions:

- the process action copies only reasoning, commentary, execution summaries, and tool details;
- each final-answer response action copies only that item's original text;
- a phase commit keeps the response copy action attached to the same scoped item identity.

## Presentation Roles

| Timeline kind or phase | Presentation | Meaning |
| --- | --- | --- |
| `Reasoning/Thinking` | Thinking | Model reasoning with faithful paragraph layout. |
| `AssistantText/Unknown` | Commentary | Uncommitted assistant text rendered in the process timeline. |
| `AssistantText/Commentary` | Commentary | Short execution commentary. |
| `ToolCall/Acting` | Tool step | A compact expandable execution record. |
| `ExecutionSummary/System` | Execution summary | A compact summary distinct from reasoning. |
| `AssistantText/FinalAnswer` | Final answer | Markdown rendered from the accumulated item text. |
| `ContextCompaction/System` | Context status | Existing context status content. |
| `SystemNotice/System` | System | Existing notice title and severity content. |

Labels derive only from item kind and the server-provided phase. Reasoning, unknown assistant text, commentary, execution summary, and tool steps remain in the alternating process list. Only `AssistantText/FinalAnswer` enters the final-answer region.

## Process Timeline Layout

The timeline uses one compact `Process` disclosure. Its summary owns the process count, duration, and turn metrics; metrics do not render as a detached footer. Expanding the disclosure reveals the alternating process items in the same container.

The presentation distinguishes roles without changing timeline state ownership:

- thinking uses a muted brain icon and gray text;
- commentary uses a message icon and normal text contrast;
- tools use an expandable execution row and wrench icon;
- the final answer uses a separate assistant icon and plain response region outside the process disclosure.

Thinking and commentary role names remain available as item semantics and DOM role markers, but are not rendered as visible headings. Their icons, text contrast, and ordered position provide the visual distinction without repeating labels above every text block.

The process rail preserves server item order. The summary collapses automatically after completion and reopens when new live process activity arrives.

The expanded process presentation is intentionally frame-light:

- the Process disclosure has no enclosing card border or shadow;
- one low-contrast divider separates the summary from the ordered rail;
- each process item is an independent block separated by a consistent one-line vertical gap;
- thinking, commentary, and system text render as plain rail content without item borders or backgrounds;
- compact tool calls use a plain execution surface without an enclosing border, background, radius, or shadow;
- the rail owns the tool icon, so compact tool steps do not repeat the inner wrench badge;
- duration and status render inline, while the result summary follows as compact metadata without a divider;
- completed status is shown once in the header and omitted from the result summary;
- list-like expanded results stay frame-free, while code and diff content may retain a dedicated content surface for readability.

The final-answer region is also frame-free. It has no enclosing border, background, corner treatment, or shadow. The assistant icon, `Final answer` typography, streaming indicator, and transcript spacing provide its visual separation. Live and history rendering use the same plain presentation.

Process, event-count, final-answer, and streaming-state labels come from the shared `chat` locale resources used by Browser and Desktop.

A compact action row follows the final-answer content at the lower left. Copy writes the answer's original text through the existing clipboard behavior. Helpful and not-helpful controls are visual placeholders only: they remain stateless and do not send feedback or persist a selection.

## Tool Step Presentation

### TUI Reference Scope

The Web tool step adopts the useful information order from the TUI command cards:

- a semantic title based on the tool's primary argument;
- compact running, completed, waiting, and failed state;
- one or two preview lines from typed result content;
- full result, input, progress, prompt, and call ID in expandable details.

The implementation remains a normal React component using existing Web state and styles.

### Single Step Owner

Each timeline tool item renders one primary execution step:

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

`ToolCallBlock` and `ToolCallCard` own this execution step. The component name remains unchanged, while the compact timeline variant renders a plain surface. The generic timeline text header wraps text items only.

The header projects status directly from `ChatToolStatus` as an icon and text rather than a badge. Duration is a separate field and appears once, using item timing with typed Bash elapsed time as a fallback. Typed result summary and preview remain below the header without repeating the status. Long semantic titles shrink and truncate before the duration, status, or disclosure control.

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
- final-answer items from explicit phase, phase commit, and history all remain timeline items keyed by `session_id`, `message_id`, and `item_id`;
- both use the same timeline labels, text presentation, tool title projection, and tool result projection at render time;
- the existing live-to-history handoff remains responsible for replacing completed live blocks with canonical history.

## Frontend File Boundary

Implementation changes are limited to:

- `talos-app/web/src/features/chat/timeline-state.ts`
- `talos-app/web/src/features/chat/__tests__/streaming.test.ts`
- `talos-app/web/src/widgets/message-list/MessageList.tsx`
- `talos-app/web/src/widgets/message-list/MessageList.test.tsx`
- `talos-app/web/src/widgets/message-list/CopyableBlock.tsx`
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
3. Unknown assistant text uses Commentary presentation before the server phase commit.
4. Final-answer content renders through Markdown after the phase commit.
5. An item started with `FinalAnswer` renders incrementally from its first delta.
6. The response copy action writes the original response text for the committed item.
7. Live and history final-answer items preserve the same scoped identity rule and presentation component.

### Tool Card

1. Bash, file, search, and Web tools produce semantic titles from representative arguments.
2. Long title previews use the configured hard cap.
3. A timeline tool item renders one primary frame-free execution step.
4. Waiting, completed, and failed states render from the actual tool status.
5. Typed Bash and failed Diff results produce their expected summaries and previews.
6. Inline status, one duration, concise result metadata, and preview occupy separate presentation fields.
7. Expanded details expose call ID, arguments, progress, prompt, and typed result content.

### Verification Boundary

1. Scoped Vitest state, pure helper, and component tests cover the changed behavior.
2. Web typecheck and lint validate frontend boundaries and types.
3. Integration tests and browser E2E are outside this implementation run.

## Breaking Change Review

There is no public API, schema, SDK, Rust, Desktop transport, or TUI breaking change.

Internal Web behavior changes are limited to:

- server phase is the only authority for final-answer presentation;
- committed unknown assistant text preserves its timeline state item and accumulated text;
- timeline prose follows original line breaks;
- commentary and final answer have explicit roles;
- timeline tools have one primary card owner;
- tool titles include a bounded semantic argument preview.

## Completion Criteria

1. Timeline prose preserves complete sentences and original line breaks.
2. Explicit final-answer text remains incremental from its first server delta.
3. Final commit preserves accumulated text and scoped state identity.
4. Thinking, commentary, execution summary, and final answer are distinct.
5. Tool calls use a single compact card with typed summary and details.
6. Browser and Desktop continue sharing the Web implementation.
7. Existing history and live lifecycle ownership remains unchanged.
8. The diff contains no runtime, protocol, SDK, Rust, TUI, archived Spec, or unrelated documentation changes.
9. Scoped unit/component tests, Web typecheck, and Web lint pass.
