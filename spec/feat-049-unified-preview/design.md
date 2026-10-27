# Session-bound unified resource preview

Status: verified

## Scope and current capabilities

Files, chat attachments and right workspace currently have disconnected opening paths. The workspace is actor-scoped but not session-scoped, and only Markdown files have a document tab. Existing `files.read` and `files.content` support registered file IDs on Desktop and Web. The Desktop bridge supports HTTP(S) external opening; it does not expose arbitrary file reads or named application launching.

This delivery uses those existing APIs only. No Rust, protocol, SDK transport or Desktop IPC capability changes. The focused interaction prototype is in the ignored root `design/unified-preview/`; this document is the implementation contract. Root `IMPLEMENTATION_PLAN.md` maps each stage to the requirement IDs below. Deferred prototype capabilities are listed at the end and are not delivery criteria.

File storage has MIME admission independently of preview. The existing default allowlist rejects HTML and ZIP, but deployment configuration can enable those recognized types. Plain text/JSON admission is not generally available through the existing detector/configuration. This delivery does not relax storage policy. Browser/file display supports content that is already legitimately registered; user upload failures remain visible. E2E explicitly enables recognized HTML/ZIP only in disposable deployment configuration.

## R1 — Session ownership and restoration

- A typed workspace context identifies a session (`sessionId`), Files library, or general workspace. Account + endpoint + context own the tab descriptors, selection and collapsed state. Same-resource opening deduplicates within its context.
- Direct Settings loads use the general context; navigating to Settings from an existing context retains it.
- Switching sessions or Files restores that context, including an empty context; never shows another context's tabs. Persist only validated presentation descriptors. Credentials, bytes, object URLs, grants and expanded/modal state are not persisted.
- Keep document/web instances mounted while hidden within the current actor, preserving live reading/form state during tab and context changes. Refresh explicitly reloads. Reloading the application reconstructs content through authenticated APIs; inner frame DOM state is not persisted.
- Persisted contexts load lazily when visited. File content queries include the owning preview instance key, so refreshing one context does not reload the same file in another context; metadata remains actor-scoped and shared.
- Fresh narrow-page loads do not reopen a persisted modal. User opening and genuine close/Escape preserve existing focus ownership. Actor/endpoint changes immediately retire current content; credential rotation does not.

## R2 — One browser shell, registered file content

- Generalize the existing embedded browser shell for webpages and Files. File address is read-only and identifies `Files / filename`; webpages retain validated HTTP(S) navigation. Share refresh, supported Open in actions and document lifecycle.
- File metadata/content use existing query/client owners and current credentials. Fetch content only for supported display formats; revoke temporary Blob URLs on replacement/unmount. No credentials in addresses, copied references or persistence.
- Browser-native HTML, images, PDF and media use sandboxed embedded content, classified by server-detected MIME (with the existing Markdown convention). Reuse the existing Markdown renderer and generic UTF-8 text for supported text MIME types. No custom PDF reader, image editor, spreadsheet or Office renderer. Unknown/unsupported content shows metadata and download in the same right tab.
- Render HTML without host bridge, same-origin access or top navigation privileges. Self-contained HTML is supported; related local files are deferred. Display that limitation. Do not guess filesystem paths or create resource endpoints.
- Refresh rereads supported content only after successful metadata refresh. Loading, denied, missing, encoding, size and download failures use existing error presentation patterns; no unauthorized fallback. Existing SDK download/content capability owns authenticated bytes.

## R3 — Entry points and client routing

| Entry | Result |
| --- | --- |
| Session history image/attachment image | Existing thumbnail + FileImage dialog on every client; unchanged |
| Files list/grid or details, any type including image | Open/reuse registered file in Files right workspace |
| Session non-image attachment or `talos-file://fileId` link | Open/reuse file in current session right workspace |
| Ordinary HTTP(S) chat/document link | Desktop default browser through existing bridge; Web right browser |
| Explicit Talos opening | Right browser on both clients |
| Explicit external website opening | Desktop default browser; Web new browser tab |
| Computer | Existing independent tab and ComputerDesktop lifecycle/control UI |

Expose a small typed opening API for file references and webpages, with an explicit preview/link intent. Reject invalid or credential-bearing URLs. Preserve session image renderer. Do not interpret arbitrary text or relative workspace paths as file IDs. Preserve other safe Markdown protocols through the existing browser anchor behavior.

Use existing Radix menu and theme/control tokens. Open in split button/menu uses Talos branding, supported external browser action where applicable, separator and Copy link. Do not advertise Chrome/Safari/iTerm selection without a real capability. Copy file references as `talos-file://fileId`; these are authenticated product references, not public sharing links. Download is separate from application selection. Close menus on context/tab/collapse changes without focusing hidden triggers.

## R4 — Automated regression ownership

Add a dedicated resource-preview E2E group selected by frontend/preview CI impact. Reuse real API and Desktop harnesses, authenticated file fixtures and installed browser runtime. Cover file formats/fallbacks, session/Files isolation and restoration, original session image dialog, independent Computer entry, client-specific link opening, refresh/retained frame state and representative narrow focus behavior. Unit tests own deterministic state, classification and routing/error branches; do not mock business APIs in product E2E.

## R5 — Acceptance and maintainability

Finish with relevant unit, type, lint, build and repository checks, scoped E2E and visual review. Run E2E only in the final acceptance phase, rerunning solely for failures or new changes. Actual commands/results/screenshots belong to ignored root `output/unified-preview/implementation/`, not this spec. Keep modules with existing owners; no parallel resource manager, renderer registry or unrelated refactor.

## Impacted areas

Web shared UI/store, existing browser and Files features, message link/attachment entry points, shared Desktop/Web frontend, E2E scenarios and CI selection. Existing Computer media, session image dialog, backend authorization and file storage remain their current owners.

## Deferred TODO — explicitly outside this delivery

Each TODO is owned by this spec and must gain an implementation plan before work starts.

- **T1 Arbitrary workspace files:** authorized local/remote path resolution and file reading require a backend/native capability; no guessing paths from chat text.
- **T2 Container HTML resource packages:** relative CSS/images/pages, local service forwarding and authenticated resource origin/proxy need a defined server capability. Current delivery supports self-contained registered HTML and already-accessible HTTP(S) sites.
- **T3 Named native applications:** installed-app discovery and Chrome/Safari/iTerm launching require Desktop IPC; current UI exposes only actual default-browser support. iTerm must never execute generated content automatically.
- **T4 Full browser navigation interception:** cross-origin/sandboxed page internal navigation/history and reliable frame-denial detection need browser-host support. This delivery controls application links and explicit address navigation; it does not inject a privileged bridge into untrusted pages.
- **T5 Extended document support:** Office conversion, archive browsing, persistent inner-frame scroll after application restart, source-line navigation and advanced encoding/large-file streaming need separate capabilities. Browser-native formats depend on client support and retain download fallback.
- **T6 File storage admission:** enabling HTML/ZIP in production requires an explicit deployment allowlist decision; general text/JSON/code storage may additionally require MIME detector/configuration support. This frontend delivery does not change defaults or claim arbitrary file storage. Preview classification alone does not grant upload permission.
