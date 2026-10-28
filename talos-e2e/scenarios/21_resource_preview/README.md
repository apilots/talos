# Unified resource preview

`01_web_preview` covers authenticated Files HTML/image/text/Markdown/archive files,
server-detected JSON/source MIME, CodeMirror text previews, archive metadata/download,
download bytes, session-message image thumbnails that open the original image in
a dialog on click, and non-image attachments in their session's right-workspace tabs,
file references, Web link routing, session/Files tab restoration, retained HTML
state, refresh, and narrow modal menu/reload behavior. It also covers the same 31 fixtures through sessionPath prepare/read, a real session presentation render, direct/outbound Worker file reads, relative/root-relative HTML assets through a configured preview origin, source-line/encoding controls and real Office→PDF rendering with reading-position restoration. Auth and model upstreams
are controlled loopback fixtures; product services, history and storage remain real.

`02_desktop_preview` reuses the production main/preload/app-server harness and
existing workspace regressions, then covers registered HTML in a real WebContentsView with protocol isolation, Files
images in the right pane, state retention, supported Open in actions and Desktop
ordinary-link dispatch without adding a preview tab, real generated JSON upload/preview and Office→PDF rendering. OS default-application
rendering is outside the harness; exact URL/policy branches have deterministic
frontend unit coverage. Session scoping uses the shared renderer and is exercised
through real sessions in the Web case. Live Computer media remains covered by
group 20 Browser Worker scenarios; Computer lifecycle and original session image
behavior also retain their owning component tests.

Run `talos-e2e/run_e2e.sh --group 21_resource_preview`. Install LibreOffice (`soffice` on PATH) for document conversion. Requirements are declared
alongside the cases; use system Chrome and an isolated Electron profile. Reports,
screenshots and logs are emitted by the runner under its configured output root.
The disposable deployment explicitly enables HTML/ZIP/text/plain/application/json MIME types using existing configuration. Unrecognized UTF-8 source files use validated text/plain; recognized text types keep their detected MIME; the server ignores upload MIME claims. Product defaults are unchanged. No external model credentials, production account or product-state injection.

Worker-local website services are outside this group: current access uses the existing Computer remote desktop. Future Web exposure requires service registration with a dedicated gateway; Talos Server is not an HTTP/port forwarding proxy. File-backed HTML/assets remain covered by the common preview API. OS-installed app rendering is local acceptance; deterministic native discovery/launch and owned-copy lifecycle are covered by owning tests.


## Format-level acceptance and Files admission

Both cases consume the same real `talos-e2e/fixtures/resource-preview/manifest.json`
through `format_matrix.py`. The eleven default Files MIME types must all upload
and render; extended formats use explicit disposable configuration. The Web case
also starts an unmodified default FileService and proves the eleven accepted
MIMEs plus rejected HTML/ZIP, before running the opt-in UI scenario. No product
allowlist is added or widened.

The matrix covers DOC/DOCX/ODT, XLS/XLSX/ODS, PPT/PPTX/ODP, standalone PDF,
PNG/JPEG/WebP/GIF/BMP, SVG source, Markdown/CSV/TXT/JSON/TypeScript/XML/HTML,
ZIP/TAR/TGZ, WAV/MP3/Ogg and WebM/MP4. Assertions inspect actual text/pages,
computed spreadsheet values, Chinese Word/slide/PDF text, image dimensions and decoded/advancing media;
opening a tab alone is not success. Each shell emits `format-matrix.json` with
per-format status, detected MIME, assertions and screenshots. See the fixture
README for codec and SVG limitations. Install LibreOffice Writer, Calc and
Impress plus CJK fonts (for example fonts-noto-cjk), not Writer alone.

Archives show package metadata and download only: no listing, extraction or member
preview. ZIP/TAR/TGZ assert byte-exact original download; TAR also captures the
narrow layout and Open-in menu. PDF evidence includes the first page and the final second-page state;
slide Chinese text is asserted on both pages. Native browser content screenshots
are referenced explicitly alongside the product shell rather than treating a
renderer-only screenshot as the complete Desktop composition.

HTML acceptance also switches the existing tab to CodeMirror source and back to browser presentation using the same prepared resource. Explicit view changes may recreate the browser surface; ordinary tab/context retention remains unchanged. Files actions use the neutral Open file label for both readable and information-only resources.

Web acceptance also follows an absolute filesystem citation with a line anchor
from a Worker Markdown document and decodes adjacent images whose names contain
literal/encoded percent signs and spaces. File citations retain absolute paths;
document assets keep browser URL-root semantics. Media assertions require actual
decoding and advancing playback. CI selects official stable Chrome, including
its media codecs, rather than a Chromium snapshot; per-case evidence records
the user agent, MIME capability, ready/network state and media error.
