# Client and SDK architecture

[Chinese](CLIENT_ARCHITECTURE_zh.md)

## SDK-first target

Every Talos product client uses a supported SDK. The TypeScript SDK, Rust SDK, and
embedded Rust SDK must expose the same product capability families and shared
domain semantics. A client selects a transport implementation; it must not
select a smaller or different product API. This is the required target contract,
not a substitute for parity tests.

## Client paths

```text
+------------------+  +------------------+  +------------------+  +------------------+
| Web UI           |  | Desktop          |  | TUI / Command    |  | Rust SDK client  |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| TS TalosClient   |  | TS product SDK   |  | Rust SDK         |  | Rust SDK         |
| shared types     |  | Client + types   |  | GatewayClient    |  | GatewayClient    |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         v                     v                     v                     v
+------------------+  +------------------+  +------------------+  +------------------+
| Browser adapter  |  | HTTP/SSE or IPC  |  | EmbeddedServer   |  | Rust AppServer   |
| HTTP + SSE       |  | personal Worker  |  | Client           |  | Client           |
+--------+---------+  +--------+---------+  +--------+---------+  +--------+---------+
         |                     |                     |                     |
         +---------------------+-----------+---------+---------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| HTTP Gateway | main JSON-RPC stdio | in-process Gateway | app-server               |
+------------------------------------------+-----------------------------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| TalosService: shared domain responses, ApiError and durable state                  |
+------------------------------------------+-----------------------------------------+
                                           |
                                           v
+------------------------------------------------------------------------------------+
| Store commit -> Timeline { Agent | SystemNotice } -> subscribers / History         |
+------------------------------------------+-----------------------------------------+
```

## Unified capability surface

SDKs are required to align on product capabilities, typed requests and
responses, errors, and stream behavior:

```text
Authentication and profile
Computer directory, owner management and availability
Models and configuration
Sessions, Turns, history, compaction, retry, and interruption
Timeline subscription, replay, recovery, and terminal state
Message queue, steering, dynamic tool results, and user actions
Projects, files, filesystem directories, memory, and workspace documents
Skills, plugins, plugin sources, and lifecycle operations
```

Language-specific naming may be idiomatic, but capability meaning, lifecycle,
error codes, ordering, pagination, optionality, and terminal outcomes stay
aligned. A capability is not complete until the TypeScript SDK facades, Rust SDK,
embedded SDK, and their product transports can all reach it.

## TypeScript SDK

| Package | Responsibility |
| --- | --- |
| `@talos/sdk-core` | Shared capability clients, domain types, errors, and generated protocol bindings |
| `@talos/sdk-browser` | HTTP, multipart upload, authentication header, and SSE transport implementation |
| `@talos/sdk-app-server` | JSON-RPC transport plus Electron main-process and typed preload IPC adapters |

Web uses `TalosClient` over HTTP/SSE to its selected management service. Desktop
uses `AppServerClient` through typed preload IPC and JSON-RPC stdio. Its bundled
app-server retains Service, Store, Coordinator and one `TalosWorker`. Verified
login enrolls or restores this same Worker's personal UUID through the central
Server. Registration does not replace the management runtime or its data.
Rust stdio and embedded clients initialize the existing host registration lifecycle
after verified login or cached-session restoration, including when Sync is disabled.
Remote dispatch stays inside `agent-client`; credentials never reach the renderer.
The production UI requires sign-in. Anonymous development keeps local execution
without registering a fictitious cloud node. Sync opt-out remains independent.
Shared generated types and parity tests keep the browser and Desktop facades aligned.

## Rust SDK and embedded SDK

`talos-sdk` owns the Rust `GatewayClient` capability contract, shared public
types, and Rust `AppServerClient`. `AppServerClient` implements the contract over
JSON-RPC stdio.

`talos-sdk-embedded` provides `EmbeddedServerClient`, which implements the same
`GatewayClient` contract over an in-process Gateway. TUI and command mode
therefore use the same compiler-checked SDK operations, responses, errors, and
Timeline behavior as protocol-backed Rust clients while avoiding a child
process or local port.

## Shared type pipeline

```text
Rust domain and current protocol definitions
                  |
                  v
       App Protocol generator
             /          \
            v            v
 JSON Schema        generated/app-protocol.ts
                         |
                         v
             @talos/sdk-core aliases and exports
                         |
                         v
          Browser SDK | App Server SDK | Web | Desktop
```

Generated protocol artifacts come from Rust definitions. Client code must not
hand-edit generated files, copy request or response DTOs, or expose transport
envelopes as domain objects.

## Client ownership

```text
Request -> transport status -> ApiResponse unwrap -> domain response / ApiError
                                                         |
                                              ACK != Turn completion

Session subscription -> replay to high water -> live delivery across Turns
         |                         |
         +-- Agent facts ----------+--> bounded live execution projection
         `-- SystemNotice ---------+--> stable-ID notice blocks
                                            |
History page -------------------------------+--> reconcile, then render
```

The request and subscription lanes can race. Match canonical message identity
and content before retiring optimistic input; an empty MessageStarted is not
proof the user's input has been rendered. Queue revision only invalidates queue
state, not a second execution-state owner. Start/retry finish their request at
ACK; stopping execution uses the actual interrupt API.

Session and Project metadata and Session History are read from the current
management host's account-scoped database. Project read/list returns retained
metadata without probing the Worker's filesystem; path validation belongs to
operations that use the directory. Removing a Project record does not require
its directory to exist or its Worker to be online; ownership and retained-file
constraints still apply.

App Server dispatches these local queries, Timeline subscriptions and metadata
deletions independently of serialized Worker commands, with separate bounded query
and deletion capacity and the original per-request authentication. Subscriptions
retain one connection-owned task registry, remove completed streams automatically,
and abort remaining tasks on unsubscribe, connection drop or shutdown.
Session deletion rejects locally observed active parent/child Turns, including
synced activity, and bounds Worker
connection setup, verification/cleanup and admission-lock acquisition to two seconds.
Cold remote deletion establishes authenticated execution access inside that preflight;
metadata and History reads never establish it. Desktop restores login and reads its
local Worker identity/cache independently of registration. The native host resumes
one owned account supervisor from the saved login at startup, successful login,
or authenticated account restoration. That supervisor verifies credentials,
registers/reconnects the Worker and starts or reuses the account's existing
`SyncHandle`. A saved document binding restores H before Worker registration RPC,
so Gateway outages cannot delay that account's synchronization transport.
Periodic maintenance and the SDK's reconnect lifecycle need no UI polling.
Later authentication refreshes its credentials without creating another
supervisor or replacing the handle. Desktop does not use status queries to enroll.

`sync/status` returns a local account-scoped snapshot of configuration, connection
observations, pending changes, errors and Computer identity. It neither starts
maintenance nor takes the registration lifecycle lock or retires registrations.
Login suspension and host shutdown cancel the supervisor before tearing down
connections. Account lifecycle mutations pause concurrent resumes until cancellation
and mutation complete. Late responses with a suspended credential cannot undo logout;
a new credential or explicit successful login resumes the same owner. Sync import/recovery
observers are owned alongside each handle and are aborted and joined before its
shutdown. Registration retirement runs in account maintenance, never in local identity
mapping or import notification processing. Explicit Sync enable/disable remain lifecycle mutations. Polling only
observes asynchronous enrollment or failure; an initial enabled account reports
`connecting` until a connection observation is available. Transport authentication
remains required. Graceful shutdown cancels account maintenance before closing
transports; process termination relies on socket closure and peer liveness expiry,
with the durable binding restored at the next authenticated startup.

An unavailable Worker is not evidence of idleness: failed verification preserves
the records and explains that idle execution could not be confirmed.
The durable Store deletion and Sync capture occur only after successful preflight,
outside that timeout. UI completion updates retained list caches immediately and
does not wait for background list reconciliation. Sync recovery runs alongside
import notifications and is triggered by connection transitions, not ordinary
pending-count updates.

The Web/Desktop transcript drains every history page on entry before subscribing
to unfinished execution. `lastStableEventId` is the opaque exclusive event cursor
of the last completed Turn returned by a history page; use the final page's value
after pagination. It is not the Session's current event high water, which can
include an unfinished Turn. The history pagination cursor is not an event cursor.
This response field requires coordinated backend and generated-client deployment;
HTTP and JSON-RPC return the same history contract. Rust SDK/TUI readers may ignore it.

Web/Desktop keeps the consumed event cursor while rendering an unfinished Turn,
then reconciles its terminal output with history without replaying completed
content. Idle Sessions query active-Turn and queue state rather than subscribe to
execution events. The active-Turn response includes `latestEventId`, the latest
durable Session event (including failures and product notices). An idle client
refreshes history when that snapshot changes, even if a remote Turn starts and
finishes between polls. This cursor is a change signal, not a replacement for
`lastStableEventId` when subscribing. Backend and generated clients must upgrade
together. Explicit maintenance can still observe Session events. The
transport's Session subscriptions continue to support delivery across Turns;
the UI owns when to close them. Turn-limited subscriptions end at that Turn's
terminal fact and can miss later product notices. History notice IDs retain their historical position; live
notices absent from the loaded page stay visible without duplication. SystemNotice
never enters LiveTurnProjection or Provider continuation. UI projections are
bounded app-owned presentation state, not SDK or database truth.

Decode untrusted errors once at ingress. HTTP authentication responses also
unwrap the application envelope; a successful HTTP status alone is not a domain
response. Health uses its own documented shape. Electron main returns plain
envelopes, preload forwards them, and the renderer reconstructs typed errors.
Connection failure during bootstrap is not proof of logout.

Keep errors structured in UI/terminal state and translate at render time through
the existing presenter. Branch on codes/typed parameters, never English sentences.
Show failed Turns even without assistant text. Public error disclosure, report
ownership and protocol 0.4 migration are defined by [Error contract](ERROR_MODEL.md).

Desktop and Web settings follow ownership of the selected Computer. The central
Server's embedded Worker is claimed by its first administrator as a personal
Computer, using direct RPC; it is not a shared Computer. A competing claim must
preserve the current owner's registration and runtime status. Registration
uses the same UUID/credential contract for embedded and standalone Workers;
`direct`/`outbound` selects transport independently of ownership. See
[deployment and authorization](../../deploy/README.md#computer-identity-transport-and-configuration-access).

- SDKs own the complete product API, typed domain objects, structured errors,
  stream contracts, and transport-neutral lifecycle semantics.
- Transport adapters own serialization, authentication transport, connection
  lifecycle, upload encoding, and event delivery mechanics.
- Clients own presentation, interaction, optimistic state, and reconnect UX.
- The server owns search, matching, parsing, permission, and durable lifecycle rules.

Desktop's app-server controller owns backend restart and subscription replay.
Replay retains the verified account ID and last delivered cursor, never a token.
After replacement it reads the current session from the existing auth owner and
uses that account's current credential through normal request validation. Missing,
rejected or different-account credentials terminate recovery explicitly; there is
no anonymous fallback. Logout, a new login, unsubscribe and controller shutdown
retire subscription contexts; retired backend events are ignored. Original
renderer unsubscribe handles continue to resolve after backend replacement.
Subscriptions accept initial replay events before the subscribe response resumes;
only delivered events advance their cursor, never an unconsumed high-water mark.
Authenticated Desktop startup restores the existing host lifecycle through
`sync/status`, using the current login credential for enrollment and owner binding.
Sign-out retires subscriptions immediately and waits only for the backend
handshake, never pending replay. Recovery completion and auth callbacks affect
only their current bridge/account generation; a retired recovery cannot clear
its replacement or rebind the previous account.

## Parity requirements

- API changes review Rust SDK, TypeScript SDK, embedded SDK, HTTP, JSON-RPC, Web,
  Desktop, and TUI impact together.
- Current protocol changes regenerate schema and TypeScript bindings with
  `just gen-app-protocol`.
- SDK parity tests compare observable behavior, not transport implementation.
- Embedded execution cannot introduce a private capability unavailable through
  the public SDK contract.
- HTTP and JSON-RPC envelopes may differ, but domain results and `ErrorCode`
  semantics agree.
- User-visible Timeline changes require rendering coverage in every affected client.

The Rust trait currently enforces parity between its app-server and embedded
implementations. TypeScript Browser and App Server facades require explicit
parity review because they are separate APIs. Documentation must not describe a
new capability as complete until those paths and their tests are aligned.

## UI implementation

- UI state reflects actual backend state. Search, matching, parsing, and selection rules belong in the backend or existing shared modules, consistent with the ownership boundaries above.
- Product UI and runtime text changes require snapshot, visual, or E2E coverage in affected clients. With `insta`, generate, inspect, and accept expected snapshots; read `*.snap.new` or equivalent diffs before acceptance.
- Use existing TUI styling helpers and the default foreground unless the design system requires a specific color. Avoid hardcoded white and churn between equivalent span/line constructions.
- Use shared text-wrapping options and line utilities for wrapping, indentation, and list prefixes instead of custom string manipulation.

### Workspace inspection

Settings → Personalization → Workspace shows the configured default workspace
for the trusted current actor. `workspace/info` (RPC) and `GET /v1/workspace/info`
(HTTP) share `WorkspaceInfoRequest` / `WorkspaceInfoResponse`, with camelCase
`workspaceId`, `rootPrefix`, and `userWorkspaceDir`. The prefix is resolved against
runtime home by configuration; the user directory uses the same resolver as
workspace documents. `workspaceDocs.info()` in the HTTP SDK and `workspaceInfo()`
in the App Server SDK expose the same response. Desktop permits the empty request
through its existing IPC guard.

This read-only operation has the same actor access as workspace documents. It
accepts no user ID, workspace selector, or client path, creates no files, and
propagates configuration/identity errors through the unified error contract.
Existing sessions retain their own workspace bindings; this endpoint neither
selects a session's workspace nor manages workspace definitions. Extend selection
only together with an authorized product workspace management path.

## Computer selection and execution ownership

`features/computers` owns the independent rail, full paginated search, selection
and management view. Both clients use `sync/status.localComputerId` to identify
the connected management host's own Worker. On Desktop this is its local Computer;
on Web it is the Server's embedded Worker, not the browser's physical machine.
Ordinary accounts on a shared center do not see the center's private embedded
Worker. An authorized local Worker is selected by default; otherwise the saved
selection or an explicit Computer selection applies.
The existing account menu remains the account owner. Cloud/local is an explicit
host setting and an offline Computer never redirects execution elsewhere.

Product requests reuse canonical optional `nodeId` fields. The UI binds environment
APIs once at composition; Session operations retain their saved identity. Query
caches use account/control-plane/resource identity, with Computer identity for
environment and list views. Drafts/recent selection are client-owned and scoped;
Projects, Sessions and Timeline remain on the control plane. A Computer switch
unsubscribes observations without cancelling its running Turn.

`AgentHarness` and `WorkerEnvironment` hide embedded/direct/outbound transport.
Worker owns config, MCP credentials and Workspace files. Remote MCP OAuth callbacks
use the control-plane relay to the original bounded Worker attempt; only transient
state-hash routing lives on the control plane. SDK and the generated PB projection
reuse the canonical business types.

`ComputerDetails` presents the canonical `NodeInfo.systemInfo` report together
with existing owner, identity, heartbeat and registration metadata. The Worker
collects it once through `talos-system-info::collect()`, and the original Node
registration/store/sync paths carry that typed object. Missing optional hardware
stays unavailable. The report never decides permissions, identity or availability.
Computer management and synchronization controls stay on the dedicated right-hand
page; the business sidebar only shows the selected Computer and its resources.


The native Browser backend scopes its current page and element references to the
selected managed/connected target kind. Switching kinds clears those observations
before the next action; tab IDs from the bundled host must never reach prepared
Chrome, or vice versa. A new Turn on the same kind may retain its current tab,
but element references still require a fresh observation. Account/control-epoch
changes also invalidate the current page, including plugin disable/re-enable after
owned website views are closed.

## Application shell and right workspace

Web and Desktop expose the same three resource-tab kinds. Computer content branches
on the execution Computer's canonical BrowserSnapshot.host/display capability,
not the viewing client's isDesktop flag. A native local Computer has no remote
desktop; Desktop can still view and, when authorized, control a dockerDesktop Worker.

AppLayout composes three full-height sibling regions: the global Computer rail,
the middle business region (Session/Project sidebar and main page), and the right
workspace. Navigation, rail, sidebar, main content and workspace remain independent
components. AppBusinessRegion only composes its own header and sidebar/main row.
Each region's header belongs inside the same vertical boundary as its content;
the workspace divider continues from its own header through tabs and content.
Do not substitute a single header spanning middle and right, or two inset cards.
Use the existing tokens and normal region structure, without measurement or a new
layout framework. The rail and region headers share the same subtle surface color;
sidebar, main and right content use small rounded corners and light borders directly
below their headers, without inset gutters or heavy color blocks.

In split view, the middle header owns PanelLeft (only its business sidebar),
back/forward, new chat and page/Session/Project context; it has no view-Computer
entry. The right region uses one top row for tabs, new-tab plus, expand/restore and
visibility controls, with a mirrored PanelRight. Do not add a separate full row
labelled Right workspace; selected content (including a webpage address bar/body)
follows directly below. When collapsed or empty, workspace controls move to the
middle header's trailing edge. A narrow/expanded workspace modal retains only its
resource/workspace controls: the original middle region remains under its owner
and becomes inert/aria-hidden, without projecting navigation or Session context
into the right region. Settings can coexist with an already-open workspace; the
MCP row still exposes only enable/disable.

SidebarComputerEntry fills the existing SidebarContainer/SidebarPrimaryNav slot
below New chat. Your computer appears only for the target snapshot's host=docker
and display.kind=dockerDesktop, opening the existing computer/nodeId tab. A Session
route uses its bound nodeId and never substitutes rail selection while loading;
other pages use the selected Computer. Desktop viewing a Docker Worker is allowed;
a native local Computer has no remote entry. The ComputerInspector browser entry
retains native bundled/My Chrome configuration and local actions.

Do not add a separate globe shortcut to the header. The rightmost workspace control shows an enabled
Plus when there are no tabs, creating an empty webpage tab through the existing
createWorkspaceWebpage action and opening New Tab. With existing tabs it shows
PanelRight and the real count in both expanded/collapsed states; clicking only
toggles workspace visibility and never adds another tab.

The middle header also splits at the sidebar/main boundary: AppNavigation aligns
with the sidebar, while AppSessionContext aligns with main content. One sidebar
width in AppBusinessRegion drives both header/body columns, transitioning between
the existing width and zero with a 200ms ease-out CSS drawer transition: the
sidebar slides left within its clipping container while main/composer expands.
Keep the sidebar's inner width fixed to avoid squeezing text;
prefers-reduced-motion removes the transition. The wide sidebar is an ordinary
aside; only the narrow overlay uses a native modal dialog. Closing immediately makes sidebar
content inert/aria-hidden and returns focus through the existing rule, without
waiting for animation. The single AppNavigation DOM remains reachable while main
header context moves aside continuously; do not make its toggle inert. The collapsed
layout has no permanent empty column or duplicate navigation entry. Geometry animation must neither remount retained webpage DOM nor
request or restore Agent/remote-input authority. The right drawer follows the same
200ms/reduced-motion/resize policy using native display/overlay transitions; hiding
immediately makes it inert/aria-hidden and retires Computer media, without timers
or delayed authority cleanup.

Initial render, refresh and passive viewport changes must not focus navigation
because the document body is active. Keep the persisted wide sidebar preference
separate from AppLayout's transient narrow drawer, which starts closed and opens
only from an explicit toggle. Close that drawer on workspace takeover or leaving
the narrow layout; components receive the owning close/toggle callbacks.
Preserve a live address input during workspace presentation changes. Restore a
cross-origin iframe through its allowed Window.focus API, without child-DOM access
or a bridge: entering a native modal clears its inner input focus in Chromium and
requires the user to refocus it, while the DOM/draft remain. Exiting the modal
preserves its current inner input focus. Genuine close/removal restores focus;
never steal a focus already moved elsewhere, and retain keyboard focus indicators.

Only the sidebar/main and middle/workspace vertical dividers resize; the 56px rail
is fixed. The existing UiSlice sidebarWidth/workspaceWidth/resizingPane fields own
ephemeral geometry, reset on account/connection scope change and excluded from
persistence. Effective widths are shared with header/body and sidebar inner width,
clamped against viewport and main-content space. Reuse AppResizeHandle for these
two keyboard separators (Left/Right, Home/End and visible focus). Pointer capture
updates width immediately with drawer transitions disabled; pointer-up, cancel,
lost capture and unmount release capture and clear transient resize state, restoring
the normal 200ms/reduced-motion policy. Resizing neither remounts resources nor
restores Agent authority, and adds no configuration service or layout framework.

| Region component | Responsibility |
| --- | --- |
| `AppTopBar` / `AppNavigation` / `AppSessionContext` | Region header primitive, sidebar-aligned navigation and main-aligned business context; no workspace geometry owner |
| `ComputerRail` | Existing Computer selection and global rail |
| `AppSidebar` | Ordinary wide aside / narrow native modal and focus boundary; reuse SidebarContainer content |
| `AppMainContent` | Main route content surface; preserve each page's business owner |
| `RightWorkspace` / `WorkspaceControls` | Right region header/geometry controls, typed tabs and content composition |

AppLayout assembles these components into the three macro regions; content kinds use separate
ComputerDesktop, MarkdownFilePreview and WebPagePreview components. Window shape
and geometry must not become a second file, browser or media authority.

The existing AppLayout composes the application shell. Its right workspace is a
presentation region; a Computer is a concrete window kind, not the workspace's
owner. `shared/store/ui-slice.ts` holds the single ephemeral set of
typed resource descriptors, active selection, visibility and expansion state.
Computer content binds an explicit nodeId; Markdown and web/local-service preview
content must use their actual authorized content owners and concrete closed types.
The closed WorkspaceWindow union is defined once in
`shared/store/workspace-window.ts`: computer/nodeId, markdown/fileId and
webpage/id/url with its own stable UI-generated tab identity. Computer/Markdown
keys derive from kind and resource identity; each new webpage tab has an independent
identity, so several tabs may visit the same URL. Address changes preserve that
identity and never merge tabs by URL. Navigation still accepts only normalized
HTTP/HTTPS addresses without embedded credentials. The existing UI slice
createWorkspaceWebpage action generates id and an empty url; navigation updates
that same descriptor without replacing its id or introducing another identity owner.
Reopening a Computer selects its existing descriptor; closing
removes it and selects an adjacent item when necessary. The window count is the
retained descriptor count, including collapsed windows. Do not count media
connections or native browser tabs, or introduce placeholder providers, arbitrary
window kinds, a new backend manager, protocol or persistent registry. Deduplicate
Computer/Markdown content by type and resource identity; preserve independent
webpage tab identities.

The three real descriptor kinds coexist as browser-style workspace tabs, one tab
per descriptor. They are separate from the Agent browser's own tabs and do not
keep all content active in the background. A trailing plus button immediately
creates and selects a real empty webpage tab, counted as open. Its page address
bar is usable in the New Tab empty state, without an initial URL dialog or error.
An empty tab has no iframe or claimed page visit; refresh/external-open stay disabled,
and its address action focuses the page address bar. When
collapsed or empty, the middle header's trailing edge exposes that entry instead; avoid duplicate
entries while the tab strip is visible. Use tablist/tab/tabpanel relationships,
a single selected tab keyboard stop, Left/Right and Home/End selection, and focus
return to the adjacent/current tab on close or the visible app entry on last close.
Only the selected visible panel is interactive. Webpage DOM remains mounted across
tab selection/collapse, hidden and inert when inactive, preserving form and scroll
state. Computer content mounts only while selected and visible; Markdown mounts
only the active item and reuses its query cache. Computer media cleanup and Human
pause rules remain unchanged. Closing a webpage tab or clearing scope removes its
iframe, rather than hiding it in a separate registry.

Resource content remains separate from shell geometry and capability authority.
Markdown uses the existing File API and MarkdownRenderer, entered from real Files
items; it adds no file store or read service. The existing FileService validates
complete `.md`/`.markdown` uploads as UTF-8 text, rejecting control characters
other than newline, carriage return and tab. Detection produces text/markdown and
still applies configured MIME allowlists, size limits and ownership checks. Neither a suffix nor a client MIME
header bypasses file admission; a configuration excluding Markdown still rejects it. Web/local-service preview is an
independent iframe, separate from the Agent's browser profile, tabs and control.
It does not attach a Browser MCP target or invoke the Desktop browser host. Honor
the destination's embedding restrictions, including CSP and X-Frame-Options. Do
not bypass them or proxy a remote Worker's localhost. Preview reachability is the
viewing client's network reachability; its localhost is not the selected Worker.
Each resource component owns loading/rendering/cleanup; existing capability owners
continue to authorize file reads, browser operations and media.
Files details opens Markdown by fileId; the tab strip's plus button, or the app
middle-header entry when collapsed/empty, immediately opens a new webpage tab with its
own address bar. Do not copy unimplemented Tools, recents or Terminal placeholders. Markdown metadata/content queries retain the existing
account/connection/file scope, error handling and explicit retry; rendering reuses
the existing MarkdownRenderer without raw HTML execution. Valid absolute HTTP(S)
links use the existing external-open path, not product-window navigation.
WebPagePreview owns its New Tab empty state, address, refresh and external-open
actions, handing URL changes back to the UI owner while preserving tab identity.
The address represents this UI's explicit submitted destination, not observable
cross-origin iframe navigation or a synchronized browser history. Its iframe uses only
sandbox allow-scripts/allow-forms and no-referrer; no same-origin, popup or
top-navigation permission, Talos token, preload or host bridge. Keep embedding
limitations visible: iframe load/error events cannot reliably identify success,
network failure or embedding refusal. Do not infer these outcomes from events;
the browser renders its actual result, with refresh/address/external-open available.

The existing app-store connection-scope subscription clears these descriptors and
layout state on account or management-endpoint identity change. Selecting a window
neither changes the Computer rail selection nor migrates a Session's execution
binding. This presentation workspace is separate from filesystem Workspace state.

Only the selected, visible Computer content mounts its viewer. Collapsing retains
window records but unmounts Computer media content; switching or closing similarly
runs the existing noVNC/SDK cleanup and invalidates pending admissions. Retained
independent webpage DOM does not retain or acquire Computer media authority. Showing content
again reads the authoritative snapshot and requests fresh media authorization.
Descriptors never hold tickets, controller epochs or sockets. A read-only viewer
ending does not pause an Agent. A Human control stream ending retains the Worker's
pause intent; shell actions never call resume implicitly. Browser configuration,
pause/takeover/release and other Computer business actions stay in the Computer
content. MCP settings retain only the built-in plugin enable/disable control.
