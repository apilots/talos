"""Production Desktop main/renderer resource acceptance with an owned CDP endpoint.

Auth alone uses the existing secretless contract fixture. The actual app-server,
File owner, preload, main process and compiled Desktop renderer handle resources.
No product store, business event, desktop bridge or component is replaced.
"""
import base64
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time
import urllib.request
from http.server import ThreadingHTTPServer

from deletion_auth_fixture import ContractAuthCluster
from fixture import ROOT, REPORT, data, free_port, require, wait_for
from registration import configure_desktop, start_ingress
from resources import PreviewPage, sidebar_focus, observe_workspace_exit, verify_workspace_exit, settle_workspace
from playwright.sync_api import expect, sync_playwright


MARKDOWN = b'''# Desktop resource acceptance

This local **Markdown** file crossed the production Desktop File API.

| Resource | Owner |
| --- | --- |
| Markdown | Native File service |
| Web previews | Independent renderer frames |

```text
desktop_resource_marker = local_file
```
'''


def prepare_entry(home):
    """Only select the disposable Electron profile before loading production main."""
    entry = home / 'production-entry'
    entry.mkdir(parents=True, exist_ok=True)
    (home / 'electron-profile').mkdir(mode=0o700, exist_ok=True)
    package = json.loads((ROOT / 'talos-app/desktop/package.json').read_text())
    (entry / 'package.json').write_text(json.dumps({
        'name': 'talos-desktop-resources-e2e', 'version': package['version'],
        'type': 'module', 'main': 'main.mjs'}))
    (entry / 'main.mjs').write_text('''import path from 'node:path';
import {pathToFileURL} from 'node:url';
import {app} from 'electron';
app.setPath('userData', path.join(process.env.TALOS_HOME, 'electron-profile'));
app.commandLine.appendSwitch('remote-debugging-address', '127.0.0.1');
await import(pathToFileURL(path.join(process.env.TALOS_RESOURCE_DIR, 'talos-app/desktop/dist/main.js')).href);
''')
    return entry


def prepare_renderer(home):
    """Build the product hash-router renderer without replacing shared build output."""
    destination = home / 'renderer'
    env = dict(os.environ, VITE_TALOS_DESKTOP='1', TALOS_WEB_BASE='./')
    with (REPORT / 'desktop-resources-renderer-build.log').open('wb') as output:
        subprocess.run(['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite',
                        'build', '--outDir', str(destination)], env=env,
                       stdout=output, stderr=subprocess.STDOUT, check=True, timeout=180)
    renderer = destination / 'index.html'
    require(renderer.is_file(), 'Desktop product renderer was not built')
    return renderer


def invoke(page, token, method, params):
    # Read-only verification follows the real public preload/IPC envelope.
    return data(page.evaluate('''async ({token, method, params}) => window.talos.invoke(
        method, params, {actorCredentials: {credential: {type: 'existing_session_token', token}}})''',
        {'token': token, 'method': method, 'params': params}))


def cdp_ready(process, endpoint):
    require(process.poll() is None, 'Owned production Desktop exited before CDP admission')
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(endpoint + '/json/version', timeout=2) as response:
        document = json.load(response)
    return bool(document.get('webSocketDebuggerUrl'))


def exercise(page, token, destination, markdown, endpoint):
    # The production main must initialize its native app-server and restore Auth
    # before the lazy Files route mounts. Match the existing native startup budget.
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    sidebar_focus(page, 'desktop')
    require(page.evaluate('typeof window.talos?.invoke') == 'function', 'Production preload bridge is missing')
    require(page.evaluate('typeof window.require') == 'undefined', 'Product renderer gained Node integration')
    identity = data(page.evaluate('async () => window.talos.workerIdentity()'))
    node_id = identity['nodeId']
    local = page.locator('[data-computer-id=' + json.dumps(node_id) + '][aria-pressed=true]')
    expect(local).to_be_visible(timeout=45_000)
    expect(local).to_have_attribute('aria-label', re.compile('This computer'))
    snapshot = invoke(page, token, 'browser/operate', {'nodeId': node_id, 'operation': {'kind': 'read'}})
    require(snapshot['enabled'] and snapshot['host'] == 'desktop', 'Native Computer did not use the actual Desktop host')
    require(snapshot['display'] == {'kind': 'unavailable', 'reason': 'nativeHost'}, 'Native Computer advertised remote desktop media')
    expect(page.get_by_role('button', name='Your computer', exact=True)).to_have_count(0)
    expect(page.locator('#talos-right-workspace')).to_have_count(0)

    # Select a real local file in the real Files page; upload goes through IPC.
    page.locator('input[type=file]').set_input_files(str(markdown))
    expect(page.get_by_role('button', name='Preview Markdown', exact=True)).to_be_visible()
    uploaded = wait_for(lambda: next((item for item in invoke(page, token, 'file/list', {'limit': 20})['data']
        if item['originalFilename'] == markdown.name), None), 'Native File upload')
    content = invoke(page, token, 'file/content', {'fileId': uploaded['fileId']})
    require(base64.b64decode(content['dataBase64']) == MARKDOWN, 'Native File content differed from the uploaded local file')
    page.get_by_role('button', name='Preview Markdown', exact=True).click()
    workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
    expect(workspace.get_by_role('heading', name='Desktop resource acceptance', exact=True)).to_be_visible()
    expect(workspace.get_by_role('table')).to_be_visible()
    expect(workspace.locator('pre code')).to_contain_text('desktop_resource_marker = local_file')
    settle_workspace(workspace)
    page.screenshot(path=str(REPORT / 'desktop-resource-markdown.png'), full_page=True)

    page.evaluate("""() => {
        window.desktopResourceUnsubscribe = window.talos.browserWorkspace.onState(state => { window.desktopResourceState = state; });
    }""")
    workspace.get_by_role('button', name='New webpage tab', exact=True).click()
    address = workspace.get_by_role('textbox', name='Web address', exact=True).last
    address.fill(destination)
    address.press('Enter')
    def targets():
        with urllib.request.build_opener(urllib.request.ProxyHandler({})).open(endpoint + '/json/list', timeout=2) as response:
            return json.load(response)

    def native_page(excluded=None):
        return wait_for(lambda: next((target for target in targets() if target['url'] == destination
            and (excluded is None or target['id'] != excluded['id'])), None), 'Native browser target')

    def native(target, kind='read', value=''):
        result = subprocess.run(['node', str(ROOT / 'talos-e2e/scripts/computer_workspace/native-resource-page.mjs'),
            target['webSocketDebuggerUrl'], kind, value], capture_output=True, text=True, check=True, timeout=30)
        return json.loads(result.stdout)

    first = native_page()
    require(native(first)['bridge'] == 'undefined', 'Website gained product IPC')
    native(first, 'fill', 'first Desktop draft')
    # Native view bounds follow keyboard resizing and responsive layout changes.
    geometry_samples = []
    def geometry():
        viewport = workspace.get_by_label('Browser page', exact=True).bounding_box()
        dimensions = native(first)
        sample = {'viewport': viewport, 'native': dimensions, 'renderer': page.evaluate('({width:innerWidth,height:innerHeight,visibility:document.visibilityState})')}
        if not geometry_samples or geometry_samples[-1] != sample:
            geometry_samples.append(sample)
            (REPORT / 'desktop-native-geometry.json').write_text(json.dumps(geometry_samples, indent=2))
        return viewport and abs(dimensions['width'] - viewport['width']) <= 1 and abs(dimensions['height'] - viewport['height']) <= 1

    wait_for(geometry, 'Native initial geometry')
    handle = workspace.get_by_role('separator', name='Resize workspace', exact=True)
    handle.focus()
    handle.press('ArrowLeft')
    settle_workspace(workspace)
    wait_for(geometry, 'Native keyboard-resized geometry')
    # Emulate narrower renderer viewports within the owned 1280px native
    # window. Wider synthetic viewports would correctly be clamped by main.
    # Native content-size clamping and physical minimized windows are exercised
    # separately by the real Electron host scenario.
    native_height = page.evaluate('innerHeight')
    for width in (980, 1280):
        address.click()
        page.set_viewport_size({'width': width, 'height': native_height})
        settle_workspace(workspace)
        wait_for(geometry, 'Native responsive geometry')
        expect(address).to_be_focused()
        require(native(first)['value'] == 'first Desktop draft', 'First native form state was lost')
    workspace.get_by_role('button', name='New webpage tab', exact=True).click()
    expect(workspace.get_by_role('tab')).to_have_count(3)
    expect(workspace.get_by_role('tab').nth(2)).to_have_attribute('aria-selected', 'true')
    address = workspace.get_by_role('textbox', name='Web address', exact=True).last
    address.fill(destination)
    address.press('Enter')
    second = native_page(first)
    require(native(second)['value'] == '', 'Same-URL native tabs shared a form')
    native(second, 'fill', 'second Desktop draft')
    tabs = workspace.get_by_role('tab')
    expect(tabs).to_have_count(3)
    tabs.nth(1).click()
    require(native(first)['value'] == 'first Desktop draft', 'First native form state was lost')
    exit_panel = observe_workspace_exit(workspace)
    workspace.get_by_role('button', name='Hide workspace (3 open windows)', exact=True).click()
    expect(workspace).not_to_be_visible()
    verify_workspace_exit(page, exit_panel, 'desktop')
    # Both hidden targets remain alive while only the product renderer is shown.
    require(native(first)['value'] == 'first Desktop draft', 'First native form state was lost')
    page.get_by_role('button', name='Show workspace (3 open windows)', exact=True).click()
    tabs.nth(2).click()
    require(native(second)['value'] == 'second Desktop draft', 'Second native form state was lost')
    settle_workspace(workspace)
    page.screenshot(path=str(REPORT / 'desktop-resource-native-tab-shell.png'), full_page=True)
    native(second, 'screenshot', str(REPORT / 'desktop-resource-native-page.png'))
    # Exercise external host reconciliation through the real preload and IPC.
    tabs.nth(2).focus()
    data(page.evaluate("""async () => {
        const state = window.desktopResourceState;
        return window.talos.browserWorkspace.request({kind:'close',sessionId:state.sessionId,tabId:state.tabs.at(-1).id});
    }"""))
    page.evaluate('window.desktopResourceUnsubscribe()')
    wait_for(lambda: all(target['id'] != second['id'] for target in targets()), 'Native tab destruction')
    expect(tabs).to_have_count(2)
    expect(tabs.nth(1)).to_be_focused()
    require(native(first)['value'] == 'first Desktop draft', 'First native form state was lost')
    tabs.nth(0).click()
    expect(workspace.get_by_role('heading', name='Desktop resource acceptance', exact=True)).to_be_visible()
    expect(workspace.get_by_role('tab', name=markdown.name, exact=True)).to_be_visible()
    require(any(target['id'] == first['id'] for target in targets()), 'Returning to Markdown disposed another native resource')
    expect(page.get_by_role('button', name='Your computer', exact=True)).to_have_count(0)
    settle_workspace(workspace)
    page.screenshot(path=str(REPORT / 'desktop-resource-markdown-retained.png'), full_page=True)
    return {'nodeId': node_id, 'nativeBrowserHost': snapshot['host'], 'nativeDisplay': snapshot['display'],
            'fileId': uploaded['fileId'], 'fileSha256': hashlib.sha256(MARKDOWN).hexdigest(),
            'twoSameUrlNativeTabs': True, 'switchAndHideRetainForms': True, 'closeDestroysNativeTarget': True, 'nativeGeometryResponsive': True, 'hostCloseRecoversFocus': True,
            'nativeRemoteDesktopEntryAbsent': True, 'trustedBridgeAbsentInPreview': True}


def run(cluster):
    cluster.start_control_plane()
    start_ingress(cluster)
    home = configure_desktop(cluster, 'desktop-resources')
    home.chmod(0o700)
    now = int(time.time() * 1000)
    auth = home / 'auth/cloud-session.json'
    auth.parent.mkdir(mode=0o700, exist_ok=True)
    auth.write_text(json.dumps({'schema_version': 2, 'cloud_api_base_url': cluster.cloud_url,
        'access_token': cluster.users['alice']['access_token'], 'token_type': 'Bearer',
        'expires_at_ms': now + 3_600_000, 'last_used_at_ms': now, 'user': cluster.users['alice']['user']}))
    auth.chmod(0o600)
    renderer = prepare_renderer(home)
    entry = prepare_entry(home)
    main = ROOT / 'talos-app/desktop/dist/main.js'
    require(main.is_file() and main.with_name('preload.cjs').is_file(), 'Build the current Desktop main and preload before acceptance')
    binary = Path(os.environ.get('TALOS_E2E_APP_SERVER_BIN', ROOT / 'talos-rs/target/debug/app-server')).resolve()
    require(binary.is_file(), 'Current product app-server binary is required')
    markdown = home / 'desktop-resources.md'
    markdown.write_bytes(MARKDOWN)
    markdown.chmod(0o600)
    electron = ROOT / 'talos-app/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
    if not electron.is_file():
        electron = ROOT / 'talos-app/desktop/node_modules/electron/dist/electron'
    require(electron.is_file(), 'Installed Electron runtime is required')
    debug_port = free_port()
    endpoint = f'http://127.0.0.1:{debug_port}'
    env = {key: os.environ[key] for key in ('PATH', 'TMPDIR', 'DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR', 'SYSTEMROOT') if key in os.environ}
    env.update(HOME=str(home), TALOS_HOME=str(home), TALOS_CONFIG=str(home / 'conf/talos.yaml'),
               TALOS_RESOURCE_DIR=str(ROOT), TALOS_APP_SERVER_BIN=str(binary),
               TALOS_DESKTOP_RENDERER_URL=renderer.as_uri())
    process = cluster.start_process('desktop-resources-production-main',
        [str(electron), str(entry), '--remote-debugging-port=' + str(debug_port)], env)
    preview = ThreadingHTTPServer(('127.0.0.1', 0), PreviewPage)
    preview_thread = threading.Thread(target=preview.serve_forever, daemon=True)
    preview_thread.start()
    try:
        wait_for(lambda: cdp_ready(process, endpoint), 'Owned production Desktop CDP', seconds=60)
        with sync_playwright() as playwright:
            browser = playwright.chromium.connect_over_cdp(endpoint)
            context = browser.contexts[0]
            context.set_default_timeout(30_000)
            expect.set_options(timeout=30_000)
            # Locale/theme are ordinary persisted user settings; auth is restored by production main.
            context.add_init_script('if (window === window.top && !localStorage.getItem("talos-web-ui")) localStorage.setItem("talos-web-ui", JSON.stringify({state:{locale:"en",uiTheme:"light"},version:0}));')
            page = context.pages[0] if context.pages else context.wait_for_event('page')
            errors, console_errors, request_failures = [], [], []
            page.on('pageerror', lambda error: errors.append(str(error)))
            page.on('console', lambda message: console_errors.append(message.text) if message.type == 'error' else None)
            page.on('requestfailed', lambda request: request_failures.append({'url': request.url, 'failure': request.failure}))
            try:
                page.wait_for_url(re.compile('^' + re.escape(renderer.as_uri())))
                page.goto(renderer.as_uri() + '#/files')
                evidence = exercise(page, cluster.users['alice']['access_token'],
                    f'http://127.0.0.1:{preview.server_port}/', markdown, endpoint)
                require(not errors, 'Unexpected production Desktop renderer errors: ' + repr(errors))
                evidence.update(status='passed', renderer=str(renderer), productionMain=str(main),
                    productionMainSha256=hashlib.sha256(main.read_bytes()).hexdigest(),
                    privateProfile=str(home / 'electron-profile'), ownedPid=process.pid,
                    remoteDebuggingPort=debug_port, createdTurn=False, frontendMocks=False)
                (REPORT / 'desktop-resources-acceptance.json').write_text(json.dumps(evidence, indent=2) + '\n')
            except Exception as error:
                failure = {'errorType': type(error).__name__, 'pageUrl': page.url, 'rendererErrors': errors,
                           'consoleErrors': console_errors, 'requestFailures': request_failures}
                try:
                    page.screenshot(path=str(REPORT / 'desktop-resources-failure.png'), full_page=True)
                except Exception as screenshot_error:
                    failure['screenshotErrorType'] = type(screenshot_error).__name__
                (REPORT / 'desktop-resources-failure.json').write_text(json.dumps(failure, indent=2) + '\n')
                raise
            finally:
                browser.close()
    finally:
        preview.shutdown()
        preview.server_close()
        preview_thread.join(timeout=5)
        cluster.stop(process)
        diagnostics = home / 'logs/talos.log'
        if diagnostics.is_file():
            (REPORT / 'desktop-resources-app-server.log').write_bytes(diagnostics.read_bytes())
    print('CASE desktop_resources_production_main_ipc_markdown_same_url_state_close_native_capability PASS', flush=True)


if __name__ == '__main__':
    cluster = ContractAuthCluster()
    try:
        run(cluster)
    finally:
        cluster.close()
