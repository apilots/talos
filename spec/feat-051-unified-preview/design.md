# Session-bound unified resource preview

Status: Phase 1 and expanded Phase 2 implemented, including the format/admission acceptance contract below.

## Phase 1 baseline and delivered scope

At the Phase 1 baseline, Files, chat attachments and right workspace had disconnected opening paths. The workspace was actor-scoped but not session-scoped, and only Markdown files had a document tab. Existing `files.read` and `files.content` support registered file IDs on Desktop and Web. The Desktop bridge supports HTTP(S) external opening; it does not expose arbitrary file reads or named application launching.

Phase 1 used those existing APIs only, without Rust, protocol, SDK transport or Desktop IPC capability changes. Expanded Phase 2 extends the shared backend/service/SDK contract as specified below. The focused interaction prototype is in the ignored root `design/unified-preview/`; this document is the implementation contract. Root `IMPLEMENTATION_PLAN.md` maps each stage to the requirement IDs below. The final section records the separately deferred website-service exposure decision.

File storage has MIME admission independently of preview. The existing default allowlist rejects HTML and ZIP, but deployment configuration can enable those recognized types. At Phase 1 completion, plain text/JSON admission was not generally available through the detector/configuration; Phase 2 adds that opt-in capability. Both phases preserve storage defaults. Browser/file display supports content that is already legitimately registered; user upload failures remain visible. E2E explicitly enables recognized HTML/ZIP only in disposable deployment configuration.

The following R1–R5 describe the Phase 1 baseline. Expanded P2-R1–P2-R5 supersede the original delivery limitations (registered-only reads, self-contained HTML and absent native application/document capabilities); session/image/Computer behavior and entry policies remain binding.

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

## Verified T6 sub-slice — Configurable text admission

This historical sub-slice completed the existing FileService text detection/configuration gap using Phase 1 preview. At that checkpoint T1/T2 were deferred; the expanded Phase 2 contract below supersedes that scope. Production allowlist selection remains a deployment decision.

### T6-R1 — Assessment and scope

Keep FileService as the metadata, MIME admission and byte owner. No new file manager, reader registry, public endpoint or protocol. Preserve session ownership, session image dialogs and Computer. Root PLAN stages map to T6-R1–T6-R4.

### T6-R2 — Authoritative bounded text detection

- Validate the full upload within the existing configured upload-size bound. Preserve existing Markdown/CSV/Office validation and magic-byte recognition of binary formats.
- Preserve existing recognized MIME types, including detected text formats, subject to full text validation. For unrecognized content, accept valid UTF-8 without binary control bytes as `text/plain`, including source code and extensionless text. Do not maintain a source-extension table.
- Valid `.json` content uses `application/json` through the existing JSON parser; malformed JSON remains ordinary text and is admitted only if `text/plain` is allowed. HTML/HTM text uses `text/html`; recognized binary bytes keep their detected MIME even under misleading text/HTML filenames.
- Extend existing configuration validation to recognize `text/plain` and `application/json`. Runtime defaults and shipped allowlists remain unchanged; documented opt-in configuration enables selected MIME types. Detection never grants upload permission.
- Do not invent general binary detection: unknown non-text bytes remain unsupported. Existing authorization, size limits and failed-upload cleanup remain authoritative.

### T6-R3 — Existing preview and automated entry paths

Use existing Files upload/read/content and browser shell for admitted HTML, JSON and source text. No new frontend rendering component. Extend group 21's existing Web/Desktop scenarios to upload real files under explicit disposable allowlists, inspect their detected MIME and preview actual stored content. Select both preview scenarios when the owning FileService admission or storage configuration code changes. Remove obsolete claims that general JSON/text cannot be configured.

### T6-R4 — Acceptance

Run scoped service/config tests through `just`, scoped Rust check/clippy/format and repository checks, then the affected Web/Desktop preview scenarios only in final acceptance. Review the diff for extra policies, extension registries or copied renderer logic. Store actual evidence in ignored root `output/unified-preview/phase-2/` and produce a self-contained review report.

## Expanded Phase 2 — One file preview capability (T1–T6)

The user has now authorized all T1–T6. The T6-only assessment above records the already verified sub-slice; it no longer limits the authorized scope. The root PLAN's current stages map to the following P2-R1–P2-R5 contract. Earlier T6 results do not count as expanded-scope acceptance.

### Layer ownership

| Layer | Owner and permitted responsibility |
| --- | --- |
| Filesystem | `talos-fs` owns capability-safe rooted range I/O through cap-std; no session, actor or transport policy. |
| Worker environment | Existing filesystem capability owns pinned directory handles, actor binding and expiry; existing direct/outbound Worker transports carry typed operations. |
| FileService | Existing registered-file authorization, MIME admission, OpenDAL and Peer source reads. Peer ranges extend its original source transport. |
| TalosService | Resolves saved session/Worker/workspace, composes registered and session sources, owns ephemeral preview prepare/read/close grants and document preparation. |
| Gateway / app-server / talos-server | Map the canonical contract to RPC/HTTP. HTTP adds browser headers and streaming; it never opens source paths or implements document readers. |
| SDK core | One typed preview client owns bounded byte streams, chunk assembly and content decoding. HTTP/RPC are transport adapters; resource URL construction is supplied by the shell. |
| Shared Web UI | One file preview component consumes the SDK contract for both sources and both shells. Session image lightbox and Computer remain in their original components. |
| Desktop shell | Adapts resource delivery to a standard Electron protocol and supplies native opening/browser capability. No source filesystem fallback or independent file API. |

### P2-R1 — Sources and owned reads (T1/T6)

Expose one `FilePreviewSource` union: `{kind: "file", fileId}` or `{kind: "sessionPath", sessionId, path}`. One prepare/read/close interface returns explicit metadata, entry resource, expiration and capabilities. Registered content retains FileService authorization. Session content uses session ownership and the saved execution Worker/workspace/project directory; clients cannot supply a node or authorization root. Worker reads canonical targets within that root, permits in-root symlinks, rejects escapes and never falls back to another machine. File links use explicit relative/absolute paths or local `file://` URLs. A line citation such as `./source.rs:27` or `/workspace/source.rs#L27` retains that file intent; scheme-looking opaque URLs are not guessed as paths. Typed range reads bound memory and transport size. Grants and temporary output are runtime-owned and never persisted as workspace windows.

### P2-R2 — Resource delivery (T2)

HTTP and app-server RPC adapt the same interface. Desktop uses a standard `talos-preview` Electron protocol that forwards reads through authenticated main→app-server; it does not read arbitrary native files. Web offers configured per-preview resource origins so browser-native relative and root-relative paths retain their preview identity. Origin/DNS/TLS configuration and scoped expiring resource authorization are part of deployment support. Path-prefix delivery may support self-contained files but must not claim root-relative support. No source rewriting, directory import or privileged untrusted-page bridge.

### P2-R3 — Opening targets and browser capability (T3/T4)

Discover/return only real native opening targets. Opening a registered or session file streams the common SDK bytes into an owned temporary copy; iTerm/terminal operations only open a directory, never execute generated files. Reuse browser host navigation/events and Electron history for supported native browsing. Return explicit capabilities for Web: it cannot enumerate installed applications, inspect arbitrary cross-origin iframe history or reliably infer frame denial. Preserve real browser external-opening alternatives. Do not fake addresses, back/forward availability or successful load state. Native application discovery/launch uses maintained app-path/open/which packages; packages with native resources load from real unpacked production dependencies in ASAR builds, preserving their module-relative resources and working directories. Computer remains independent.

### P2-R4 — Shared document capability (T5)

Office formats share LibreOffice headless conversion to PDF; PDF output shares the same browser/viewer path. Use mature archive libraries for bounded directory/member reads; member preview reuses the common resource interface. Use a mature text/PDF component where browser-native viewing is insufficient. Support text line location/encoding and bounded reads; persist only controlled reading positions. Never claim restoration of arbitrary cross-origin scripts/forms after restart. Explicitly report unavailable conversion/platform capabilities; no per-format public API or renderer registry.

### P2-R5 — Verification and handoff

Complete owning Rust/frontend/config/protocol checks, schema generation, CI selection and final real Web/Desktop resource tests. Verify HTML relative/root-relative assets, session-pinned reads, target launching without code execution, actual document rendering, lifecycle/range bounds and original image/Computer regressions. Store actual evidence only in ignored output. Document hard platform limits and any unverified acceptance separately from delivered behavior.

### P2-R5 format and admission acceptance matrix

Keep Files admission and preview capability separate. `storage.files.allowed_mime_types`
and the existing FileService MIME/size validator decide whether bytes may enter
Files; the preview interface reuses the stored authoritative metadata and source
permissions. Changes to upload admission do not retroactively replace existing file read authorization. Do not create a second upload allowlist or infer permission from a
viewer. Session workspace files are read under saved session/Worker root access,
without uploading to Files; their resource bytes use the same preview interface.

Exercise all eleven default MIME types through real upload and rendering:
DOC, DOCX, XLS, XLSX, PPTX, PDF, PNG, JPEG, WebP, CSV and Markdown. A separate
unmodified-default admission scenario must reject HTML/ZIP even though they can
be previewed after explicit opt-in. Never silently broaden shipped configuration.

Additional configured coverage includes ODT/ODS/ODP/PPT, GIF/BMP, plain text,
JSON/source/XML/HTML, ZIP/TAR/TGZ and representative decoded audio/video
(WAV PCM, MP3, Ogg Vorbis, WebM VP8, MP4 H.264). Share fixtures and assertions
between Web/Desktop. Assert actual document text/pages and spreadsheet formula
values, Chinese Word/slide/PDF text, image decode/dimensions, archive member content and advancing media,
not only a visible frame or tab. A browser media family does not guarantee every
codec/container on every platform. Files currently detects SVG as plain text;
record source display explicitly rather than claiming visual SVG Files preview.
Worker HTML SVG assets retain browser-native rendering.

## Coverage tracking

T1–T6 are now in the authorized expanded plan, not silently deferred. Production deployments still retain explicit MIME policy; preview ability never grants filesystem or upload access. Current T6 opt-in admission is already implemented. Expanded delivery follows P2-R1–P2-R5 above; actual checks, local acceptance and limitations belong to the ignored root evidence directory.

### Preview origin deployment

- `storage.files.preview_base_url` is an optional dedicated origin such as `https://preview.example.com/`. Route `*.preview.example.com` to the same talos-server and terminate TLS with the matching wildcard certificate. Preserve the original `Host` header; do not rewrite preview hosts to the product host.
- Each prepared resource gets `<previewId>.preview.example.com`. The browser then resolves `/assets/...`, relative CSS/JS/media and HTML subpages natively. Requests carry only the ephemeral scoped preview identifier; product account credentials are not embedded in URLs or exposed to the document.
- Without that deployment option, the SDK uses `/v1/file-previews/resources/<id>/<path>` for self-contained/relative resources. Root-relative resource URLs require the dedicated origin. Desktop's standard `talos-preview://<id>/...` scheme provides this origin identity locally without an HTTP server.
- Both resource adapters implement GET/HEAD, single byte ranges, bounded reads, no-store, no-referrer, nosniff and an opaque document sandbox. Grants expire after 15 minutes; refresh prepares a new grant, and closing the tab withdraws its existing grant. A reload does not persist authority.

### Document resource contract

`FilePreviewDescriptor.document` is either null or an explicit PDF/archive/unavailable capability. `entryPath` still addresses the original source. PDF output and archive entries publish their own resource paths; clients pass those paths back unchanged. Archive entries also provide a display `path`, so UI never parses a reserved transport prefix. Original and derived reads retain the same source authorization and grant lifetime. Archive resource paths use browser-normalized components while the archive owner retains the original member names. Listing uses filename metadata hints; selecting a member performs the common metadata-only read to classify its bounded byte prefix, using the same MIME owner as Worker files. Listing never decompresses all members. The workspace tab and Files selection retain the archive name; directory and member views identify the source archive explicitly. Member navigation displays the complete in-archive path and a return-to-directory action. Toolbar location and download/Open-in targets refer to the currently displayed member.

Office conversion uses the maintained [LibreOffice](https://www.libreoffice.org/release-notes/) stable distribution (`soffice` on PATH; current latest stable line 26.8.1). Talos wraps its CLI and does not implement Office parsing. Each conversion uses an isolated temporary input/output/profile, a bounded process lifetime and the configured upload byte budget; outputs are served by the existing resource adapters. Missing conversion support leaves original-file download available. The conversion environment must provide Writer, Calc and Impress plus fonts needed by its documents (for example Noto CJK for Chinese). Fonts remain a standard converter/deployment dependency, without a Talos font renderer, text rewriting or per-format configuration. Archive indexing uses ZIP/TAR libraries, never extracts into a workspace, limits listing to 10,000 files, and bounds gzip traversal/member bytes to the configured upload budget.

PDF rendering uses [React-PDF 11](https://github.com/wojtekmaj/react-pdf/releases) and [PDF.js 6.4.299](https://github.com/mozilla/pdf.js/releases), with the wrapper and worker resolved to the same PDF.js version. Bundle its CMaps, fonts and WASM locally through the maintained Vite static-copy plugin; documents are not uploaded to an external viewer.

Controlled text/PDF/Markdown position belongs to the existing session/Files window snapshot. CodeMirror owns text layout/line navigation, TextDecoder owns encoding, and React-PDF/PDF.js owns PDF rendering. Browser-native HTML/media remains on the existing browser surface; arbitrary page script/form state is not serialized.


## Deferred website-service exposure — Product decision

This decision distinguishes a running website service from reading files. It does not defer the unified file-preview contract above.

- Desktop can preview a website reachable from the user's computer, including a locally running service, through the embedded browser or the user's browser. The address must actually be reachable from that computer; opening a URL does not create connectivity to a remote Worker.
- Web UI with talos-server and a remote Talos Worker does not expose Worker-local website services in this phase. Use the existing Computer remote desktop to access those services for now. Public/reachable HTTP(S) links retain the existing Web embedded-browser policy.
- A future website-service capability must start the service on its execution Worker, register it with a dedicated exposure gateway, and return a reachable preview address. Service lifecycle, registration, authorization, routing and withdrawal belong to that future design. Talos Server must not implement a proxy/port-forwarding gateway for this purpose.
- TODO: design and integrate that Worker service-registration/exposure-gateway flow in a separate phase before promising Web UI preview of Worker-local websites. No HTTP-service proxy, port forwarding or gateway implementation is part of the current T1–T6 acceptance.
- Pure files on local or remote Workers, including HTML and its file-based CSS/JS/images/subpages, continue through the current unified prepare/read/close API and shared Web/Desktop preview rendering. The resource-origin adapter serves authorized file bytes; it does not forward website-service requests.
