# Computer Topology HTML Design

Standalone design prototype on branch `design/computers-topology`, based on fetched `origin/main` at `696e4699`. It does not alter the existing React page or product behavior. All device data is fictional and all management controls affect only in-memory demo state.

Open `index.html` directly, or run from this worktree root:

```sh
python3 -m http.server 4318 --bind 127.0.0.1 --directory prototypes/computers
```

Visit <http://127.0.0.1:4318>. The shared task plan lives in the sibling Sync repository at `/Users/xujiang/data/ailabs/monorepo/talos-sync/IMPLEMENTATION_PLAN.md`, as requested.

## Interaction and visual decisions

- Preserve Talos's 56 px Computer rail, IBM Plex font stack, dedicated Computer management ownership and separate local account-sync control. Follow the reference's rounded device cards and curved links, with two fixed infrastructure nodes.
- Gray bidirectional lines represent registration/execution/result transport through the Control Plane. Violet dashed bidirectional lines represent committed account facts through Sync. Animated dots illustrate direction only; they are not live traffic observations.
- Desktop cards combine the management host and its embedded Worker on one physical Computer. Independent Worker cards connect only to the Control Plane. Control Plane has a Sync edge in this example because its management host has account sync enabled. This edge is conditional in a real deployment.
- A fixed service is not a Computer. The initial example has six Computers, five online, two online Desktop management peers plus the Control Plane management peer, and one offline Desktop. Three pure Workers do not contribute to Sync peer count.
- Topology and list share search, status filtering and selection. Details persist when switching views. Selecting a node emphasizes its links; selecting either infrastructure node explains its ownership. Offline links stay visible and muted; transport availability and synchronization are separate statuses.
- The local sync switch removes active sync transmission for this host while preserving all execution relationships; the management peer count changes from three to two. It does not change other devices' preferences.
- Rename and remote enable/disable are local demo actions. Connect and mode switching explain the corresponding existing product paths. Computer entry now opens the corresponding workspace layout inside this prototype. No enrollment credential, RPC, account mutation or mode migration runs.
- Narrow viewports keep a horizontally scrollable topology with an explicit hint; list mode provides a denser alternative. Device details move below the graph after selection. No whole-page horizontal overflow is intended.

## Required data before product implementation

| View content | Owner / available evidence |
| --- | --- |
| Computer identity, system report, owner and online state | Existing authorized Computer directory / canonical `NodeInfo`. Preserve user/account visibility and backend pagination. |
| Current host identity and sync preference | Existing `sync/status`, `localComputerId`, host connection settings and current account selection. Local means the management host's Worker, not the browser's physical machine. |
| Worker execution route and health | Existing Talos registration/lease and Direct/Connect transport owners. Sync cannot determine executable availability. The specific per-row transport display may need an authorized projection. |
| Remote manager relationships and sync health | Future authorized account-scoped peer projection, authoritative peer-to-management-host/Computer mapping, role, freshness and directional telemetry. Existing presence lacks remote queue depth, byte rates, role and latency. |
| Public global counters | Sync Metrics, which must not disclose account-private topology or become the product authorization boundary. |

Follow [Computer UI ownership](../../docs/architecture/CLIENT_ARCHITECTURE.md#computer-selection-and-execution-ownership) and [the existing Sync integration model](../../spec/feat-044-talos-sync/design.md). Existing [ComputersPage](../../talos-app/web/src/pages/computers/ComputersPage.tsx) remains untouched.

The eventual graph needs explicit unknown, stale, disconnected and disabled states. Missing peer telemetry must not be interpreted as no sync or a fully caught-up peer. Disabling remote access does not itself disconnect an existing transport or disable account sync. File contents and execution RPC are not account-sync traffic. Graph loading/pagination, large fleet aggregation and production auto-refresh are intentionally outside this visual prototype.

## Observed verification

2026-10-14: Chrome rendered at 1920 px, 1440 px and 390 px. Verified shared list/topology search, node details, sync-only layer filtering (three dimmed pure Workers and no Worker-to-Sync edges), local sync off (six execution relationships preserved and two management peers), list selection, offline filtering and empty results. Mobile list selection exposes details below the panel. No browser warning/error was recorded during these interactions.

Screenshots are transient evidence in ignored `output/ui-design/`. The shared task plan records final syntax/asset/diff checks. These checks validate the HTML design only, not product transport, synchronization or backend API behavior.


## Follow-up: align with the existing application shell

The approved management content keeps its typography, topology/list switch, nodes, flow layers and detail panel. The surrounding navigation now follows these existing owners:

- [AppLayout](../../talos-app/web/src/app/layouts/AppLayout.tsx): persistent 56 px rail; `/computers` occupies the entire area to its right without the project/conversation sidebar. Management scrolls inside the available application height.
- [ComputerRail](../../talos-app/web/src/features/computers/ComputerRail.tsx): Home, three quick Computer icons, searchable overflow, management entry, and account at the bottom. Quick entries prioritize the local and currently entered Computer.
- [SidebarContainer](../../talos-app/web/src/features/chat/components/SidebarContainer.tsx) and [Sidebar](../../talos-app/web/src/widgets/sidebar/Sidebar.tsx): entering a Computer exposes its own 296 px project/conversation sidebar, with new conversation, memory, plugins and files. The account entry remains in the outer rail.
- [ChatHomePage](../../talos-app/web/src/pages/chat/ChatHomePage.tsx): the workspace example follows the existing centered prompt/composer structure and its Chinese heading. Model and permission controls are layout placeholders, not live configuration.

`shell.css` and `shell.js` own the prototype application shell; `styles.css` and `app.js` continue to own the approved management design. Rail entry (`computerId`) and management inspection (`selected`) are independent. The management detail's Computer entry action enters that specific Computer. Returning to management preserves the previous management list/topology and filter state. Sidebar projects and conversations are per-Computer fixtures, and draft/project selections stay isolated by Computer in memory. Native dialog semantics provide the resource drawer below the existing 1024 px breakpoint; it can close via Escape, its collapse action or the backdrop.

Follow-up verification on 2026-10-14: at 1440 px, measured rail width 56, resource width 296 and workspace start x=352. Switching Studio → local Desktop → Studio preserved only Studio's draft in that scope. Inspected gpu-worker while Studio remained entered, then entered gpu-worker from its detail action and observed its own resources. Overflow search selected the offline MacBook Air; its workspace stayed selected and disabled task entry. Sidebar collapse/expand and return to full-width management passed. At 390 px, management and workspace document widths both equaled 390; the resource drawer opened and closed, and list mode remained usable. No browser warning/error was recorded.

This remains HTML design only. The real React layout, backend state, tasks and Sync monitoring prototype are unchanged.
