# Unified resource preview

`01_web_preview` covers authenticated Files HTML/image/text/Markdown/unknown files,
download bytes, real session history image dialogs and non-image attachments,
file references, Web link routing, session/Files tab restoration, retained HTML
state, refresh, and narrow modal menu/reload behavior. Auth and model upstreams
are controlled loopback fixtures; product services, history and storage remain real.

`02_desktop_preview` reuses the production main/preload/app-server harness and
existing workspace regressions, then covers registered HTML isolation, Files
images in the right pane, state retention, supported Open in actions and Desktop
ordinary-link dispatch without adding a preview tab. OS default-application
rendering is outside the harness; exact URL/policy branches have deterministic
frontend unit coverage. Session scoping uses the shared renderer and is exercised
through real sessions in the Web case. Live Computer media remains covered by
group 20 Browser Worker scenarios; Computer lifecycle and original session image
behavior also retain their owning component tests.

Run `talos-e2e/run_e2e.sh --group 21_resource_preview`. Requirements are declared
alongside the cases; use system Chrome and an isolated Electron profile. Reports,
screenshots and logs are emitted by the runner under its configured output root.
The disposable deployment explicitly enables recognized HTML/ZIP MIME types using existing configuration. Product defaults are unchanged. No external model credentials, production account or product-state injection.

Deferred capability coverage follows feat-049 TODO T1–T6: arbitrary workspace
paths, container relative-resource serving, named applications, complete browser
navigation interception and extended document conversion are not simulated as
implemented product features.
