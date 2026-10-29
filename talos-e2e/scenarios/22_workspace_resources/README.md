# Lightweight workspace resources

This group owns the existing Web and native Desktop resource-tab journeys. It
uses local HTTP pages and Markdown, without Docker or the full document-format
matrix. Browser/MCP runtime belongs to group 20 and format preview to group 21.

```bash
just e2e-group 22_workspace_resources
just e2e-case 22_workspace_resources/01_resource_tabs
```

| Case | Product journey | Assertions |
| --- | --- | --- |
| `01_resource_tabs` | Web product layout → two independent local-service previews → local Markdown → close/account switch | Initial/refresh sidebar state does not steal focus, genuine keyboard focus remains visible, and closing restores it; same-URL instances keep separate form state; switching/collapse preserves it; keyboard and modal focus work; close/logout destroys instances; real File API renders Markdown and rejects non-owner metadata/content. |
| `02_desktop_resources` | Actual production Desktop main and renderer → shared preview/Markdown resources | Native IPC, sidebar initial/refresh focus and local File import/render work in the shipped client; same-URL previews retain independent state; local Computer has no Docker desktop entry. This is the native integration slice, not a duplicate Web focus/authorization suite. |

Responsive checks retain address/iframe DOM and drafts. Address focus survives
layout transitions; native modal exit restores the browsing context without a
cross-origin product bridge. Each scenario checks real Server/File APIs; only
upstream Auth is an explicit contract fixture. Desktop uses the production
main/preload/renderer and app-server.

Implementation and reusable workspace harness functions live in
`talos-e2e/scripts/workspace_resources/`. Group 21 also consumes those helpers,
so helper-directory changes select both groups; case-wrapper or requirements
changes select only group 22. Existing Computer/Auth setup remains reused from
`computer_workspace`; no duplicate fixture or runner is introduced.

Requirements are declared alongside these cases. Web needs installed Chrome;
Desktop additionally needs Electron's display/system libraries and Caddy. Use
`TALOS_E2E_SERVER_BIN` and `TALOS_E2E_APP_SERVER_BIN` to supply current binaries.
The runner discovers all shell cases and validates their existing requirements;
CI derives execution environment from those same declarations, not case names.
