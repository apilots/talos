"""Hosted product workspace acceptance; real Server/File APIs and isolated system Chrome.

Only Auth is an explicit contract fixture. Web previews use a deterministic local
page; no models, Workers, external sites, frontend mocks or store injection are used.
Computer media lifecycle remains in Browser Worker acceptance and component tests.
"""
import json
import os
import re
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from deletion_auth_fixture import ContractAuthCluster
from deletion import public
from fixture import ROOT, REPORT, data, free_port, require, wait_for
from playwright.sync_api import sync_playwright, expect


class PreviewPage(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        body = b'<!doctype html><title>Local preview</title><label>Draft<input aria-label="Draft"></label>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)


def seed(context, token):
    # Normal persisted locale/auth admission; resource state is created only by UI.
    context.add_init_script('if (window === window.top) sessionStorage.setItem("talos-web-auth-token", ' + json.dumps(token) + ');')
    context.add_init_script('if (window === window.top && !localStorage.getItem("talos-web-ui")) localStorage.setItem("talos-web-ui", JSON.stringify({state:{locale:"en",uiTheme:"light"},version:0}));')


def sidebar_focus(page, prefix):
    """Verify real refresh/focus behavior without suppressing keyboard outlines."""
    expanded = page.get_by_role('button', name='Collapse sidebar', exact=True)
    collapsed = page.get_by_role('button', name='Open sidebar', exact=True)
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    expect(expanded).to_be_visible()
    expect(expanded).not_to_be_focused()
    page.reload()
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    expect(expanded).to_be_visible()
    expect(expanded).not_to_be_focused()
    page.screenshot(path=str(REPORT / (prefix + '-sidebar-default.png')), full_page=True)
    # Reach the toggle with an actual Tab key, rather than equating programmatic
    # focus with a user's keyboard navigation or hiding the browser's outline.
    expanded.focus()
    page.keyboard.press('Shift+Tab')
    page.keyboard.press('Tab')
    expect(expanded).to_be_focused()
    require(expanded.evaluate('node => node.matches(":focus-visible") && getComputedStyle(node).outlineStyle !== "none" && parseFloat(getComputedStyle(node).outlineWidth) > 0'),
            'Sidebar keyboard focus lost its visible focus indicator')
    expanded.press('Enter')
    expect(collapsed).to_be_focused()
    page.reload()
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    expect(collapsed).to_be_visible()
    expect(collapsed).not_to_be_focused()
    collapsed.click()
    expect(expanded).to_be_visible()
    original = page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
    page.set_viewport_size({'width': 820, 'height': original['height']})
    expect(collapsed).to_be_visible()
    page.reload()
    expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
    expect(collapsed).to_be_visible()
    expect(collapsed).not_to_be_focused()
    expect(page.get_by_role('dialog', name='Projects and chats', exact=True)).to_have_count(0)
    page.screenshot(path=str(REPORT / (prefix + '-sidebar-narrow-default.png')), full_page=True)
    collapsed.press('Enter')
    sidebar = page.get_by_role('dialog', name='Projects and chats', exact=True)
    expect(sidebar).to_be_visible()
    page.keyboard.press('Escape')
    expect(collapsed).to_be_focused()
    page.set_viewport_size(original)
    expect(expanded).to_be_visible()


def workspace_focus_resize(page, workspace, address, frame, draft):
    """Preserve address focus, iframe context/state, and input focus on modal exit."""
    original = page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
    observations = []
    value = draft.input_value()
    def observe(kind, width, stage):
        return {'kind': kind, 'width': width, 'stage': stage,
            'outerFrameActive': frame.evaluate('frame => document.activeElement === frame'),
            'innerInputActive': draft.evaluate('input => document.activeElement === input'),
            'childDocumentHasFocus': draft.evaluate('() => document.hasFocus()'),
            'draftRetained': draft.input_value() == value,
            'sameFrameConnected': frame.evaluate('frame => frame.isConnected')}
    try:
        for kind, target in (('address', address), ('iframe', draft)):
            for width in (820, 1560):
                # Native modal entry may retire a cross-origin inner input. A
                # user reselects it before the separately asserted modal exit.
                settle_workspace(workspace)
                target.click()
                expect(target).to_be_focused()
                observations.append(observe(kind, width, 'after-click-before-resize'))
                page.set_viewport_size({'width': width, 'height': original['height']})
                page.wait_for_function('modal => document.querySelector("#talos-right-workspace")?.matches(":modal") === modal', arg=width < 1024)
                settle_workspace(workspace)
                observations.append(observe(kind, width, 'after-resize'))
                if kind == 'address' or width >= 1024:
                    expect(target).to_be_focused()
                if kind == 'iframe':
                    require(frame.evaluate('frame => document.activeElement === frame'), 'Responsive workspace stole iframe context focus')
                    require(draft.evaluate('() => document.hasFocus()'), 'Responsive iframe context did not regain focus')
                expect(draft).to_have_value(value)
                require(frame.evaluate('frame => frame.isConnected'), 'Responsive workspace replaced the live preview')
            expect(workspace).to_be_visible()
    finally:
        (REPORT / 'workspace-focus-resize.json').write_text(json.dumps(observations, indent=2) + '\n')
        page.set_viewport_size(original)


def settle_workspace(workspace):
    """Capture settled layout, while transition evidence remains a separate check."""
    workspace.evaluate('''async node => {
      node.getBoundingClientRect();
      await Promise.all(node.getAnimations().map(animation => animation.finished.catch(() => {})));
    }''')


def observe_workspace_exit(workspace):
    """Observe native transition events without replacing UI state or its clock."""
    panel = workspace.element_handle()
    panel.evaluate('''node => {
      node.drawerEvents = [];
      for (const type of ['transitionrun', 'transitionend']) node.addEventListener(type, event => {
        if (event.target === node) node.drawerEvents.push({type, property: event.propertyName,
          elapsed: event.elapsedTime, width: node.getBoundingClientRect().width});
      });
    }''')
    return panel


def verify_workspace_exit(page, panel, prefix):
    reduced = page.evaluate('matchMedia("(prefers-reduced-motion: reduce)").matches')
    if not reduced:
        page.wait_for_function('node => node.drawerEvents.some(event => event.type === "transitionend" && event.property === "width")', arg=panel, timeout=3000)
        events = panel.evaluate('node => node.drawerEvents')
        require(any(event['type'] == 'transitionrun' and event['property'] == 'width' and event['width'] > 0 for event in events), 'Workspace skipped the shrinking drawer transition')
        require(any(event['type'] == 'transitionend' and event['property'] == 'width' and abs(event['elapsed'] - .2) < .02 for event in events), 'Workspace drawer did not use its 200ms transition')
    else:
        events = panel.evaluate('node => node.drawerEvents')
    require(panel.evaluate('node => node.inert && node.getAttribute("aria-hidden") === "true"'), 'Exiting workspace retained interaction')
    (REPORT / (prefix + '-drawer-transition.json')).write_text(json.dumps({'reducedMotion': reduced, 'events': events}, indent=2) + '\n')


def run(cluster):
    cluster.start_control_plane()
    data(public(cluster, '/v1/computers', 'POST', {'displayName': 'Workspace test Computer', 'credentialExpiresAt': 2_000_000_000}))
    port = free_port()
    cluster.start_process('web', ['pnpm', '--dir', str(ROOT / 'talos-app'), '--filter', 'talos-web', 'dev', '--host', '127.0.0.1', '--port', str(port), '--strictPort'], dict(os.environ, VITE_API_PROXY_TARGET=cluster.url))
    url = f'http://127.0.0.1:{port}'
    wait_for(lambda: urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=2).status == 200, 'Product Web server')
    preview = ThreadingHTTPServer(('127.0.0.1', 0), PreviewPage)
    preview_thread = threading.Thread(target=preview.serve_forever, daemon=True)
    preview_thread.start()
    destination = f'http://127.0.0.1:{preview.server_port}/'
    try:
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
            context = browser.new_context(viewport={'width': 1560, 'height': 1000}, locale='en-US')
            seed(context, cluster.users['alice']['access_token'])
            page = context.new_page()
            errors = []
            page.on('pageerror', lambda error: errors.append(str(error)))
            page.goto(url + '/files')
            expect(page.get_by_role('button', name='New workspace tab', exact=True)).to_be_visible()
            sidebar_focus(page, 'workspace')
            expect(page.get_by_role('button', name='Your computer', exact=True)).to_have_count(0)
            page.get_by_role('button', name='New workspace tab', exact=True).click()
            workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
            expect(workspace).to_be_visible()
            expect(workspace.get_by_role('tab')).to_have_count(1)
            expect(workspace.get_by_role('tab')).to_be_focused()
            settle_workspace(workspace)
            require(workspace.bounding_box()['width'] == 480, 'Workspace opening default compressed the conversation area')
            address = workspace.get_by_role('textbox', name='Web address', exact=True)
            address.fill(destination)
            address.press('Enter')
            first = workspace.locator('iframe').element_handle()
            first_frame = first.content_frame()
            first_frame.get_by_role('textbox', name='Draft').fill('first tab draft')
            workspace_focus_resize(page, workspace, address, first, first_frame.get_by_role('textbox', name='Draft'))
            handle = workspace.get_by_role('separator', name='Resize workspace', exact=True).bounding_box()
            opening_width = workspace.bounding_box()['width']
            page.mouse.move(handle['x'] + handle['width'] / 2, handle['y'] + 100)
            page.mouse.down()
            page.mouse.move(handle['x'] + handle['width'] / 2 - 96, handle['y'] + 100, steps=6)
            page.mouse.up()
            settle_workspace(workspace)
            resized_width = workspace.bounding_box()['width']
            require(abs(resized_width - opening_width - 96) <= 1, 'Dragging did not adjust the workspace width')
            # A second tab with the same URL has independent DOM and form state.
            workspace.get_by_role('button', name='New webpage tab', exact=True).click()
            workspace.get_by_role('textbox', name='Web address', exact=True).last.fill(destination)
            workspace.get_by_role('textbox', name='Web address', exact=True).last.press('Enter')
            expect(workspace.get_by_role('tab')).to_have_count(2)
            second = workspace.locator('iframe').nth(1).element_handle()
            second_frame = second.content_frame()
            expect(second_frame.get_by_role('textbox', name='Draft')).to_have_value('')
            second_frame.get_by_role('textbox', name='Draft').fill('second tab draft')
            tabs = workspace.get_by_role('tab')
            tabs.nth(1).focus()
            tabs.nth(1).press('ArrowLeft')
            expect(tabs.nth(0)).to_be_focused()
            expect(first_frame.get_by_role('textbox', name='Draft')).to_have_value('first tab draft')
            require(first.evaluate('(frame) => frame.isConnected'), 'Switching replaced the first preview DOM')
            exit_panel = observe_workspace_exit(workspace)
            workspace.get_by_role('button', name='Hide workspace (2 open windows)', exact=True).click()
            toggle = page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)
            expect(toggle).to_be_focused()
            verify_workspace_exit(page, exit_panel, 'workspace')
            require(first.evaluate('(frame) => frame.isConnected'), 'Collapsing destroyed a preview')
            toggle.click()
            settle_workspace(workspace)
            require(abs(workspace.bounding_box()['width'] - resized_width) <= 1, 'Reopening discarded the user-resized width')
            expect(first_frame.get_by_role('textbox', name='Draft')).to_have_value('first tab draft')
            tabs.nth(0).press('End')
            expect(tabs.nth(1)).to_be_focused()
            expect(second_frame.get_by_role('textbox', name='Draft')).to_have_value('second tab draft')
            # Exercise the browser's real modal/top-layer/focus behavior, not jsdom.
            workspace.get_by_role('button', name='Expand workspace', exact=True).click()
            require(workspace.evaluate('(dialog) => dialog.matches(":modal")'), 'Expanded workspace is not a native modal dialog')
            require(workspace.evaluate('node => getComputedStyle(node, "::backdrop").backgroundColor') == 'rgba(0, 0, 0, 0)', 'Expanded workspace tinted the Computer rail')
            page.keyboard.press('Escape')
            expect(workspace).not_to_be_visible()
            expect(page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)).to_be_focused()
            page.get_by_role('button', name='Show workspace (2 open windows)', exact=True).click()
            expect(workspace.get_by_role('button', name='Expand workspace', exact=True)).to_be_visible()
            require(not workspace.evaluate('(dialog) => dialog.matches(":modal")'), 'Reopening retained expanded modal state')
            settle_workspace(workspace)
            page.screenshot(path=str(REPORT / 'workspace-same-url-tabs.png'), full_page=True)
            workspace.get_by_role('button', name=re.compile('^Close ')).nth(1).click()
            require(not second.evaluate('(frame) => frame.isConnected'), 'Closing a tab kept its iframe alive')
            expect(workspace.get_by_role('tab')).to_have_count(1)
            expect(first_frame.get_by_role('textbox', name='Draft')).to_have_value('first tab draft')
            # Upload and open Markdown through the actual product and File owner.
            with page.expect_response(lambda response: response.url.endswith('/v1/files') and response.request.method == 'POST') as uploaded:
                page.locator('input[type=file]').set_input_files({'name': 'workspace-review.md', 'mimeType': 'text/markdown', 'buffer': b'# Workspace acceptance\n\nPrivate **Markdown** rendered by the product.\n'})
            file_id = uploaded.value.json()['data']['file']['fileId']
            metadata = public(cluster, '/v1/files/' + file_id, user='bob')
            require(metadata.get('error', {}).get('code') == 'resourceNotFound', 'Non-owner read private Markdown metadata')
            denied = context.request.get(cluster.url + '/v1/files/' + file_id + '/content', headers={'Authorization': 'Bearer ' + cluster.users['bob']['access_token']})
            require(denied.status == 404, 'Non-owner read private Markdown content')
            with page.expect_request(lambda request: '/v1/files/' + file_id + '/content' in request.url) as content_request:
                page.get_by_role('button', name='Open file', exact=True).click()
            require(content_request.value.headers.get('authorization') == 'Bearer ' + cluster.users['alice']['access_token'], 'Product Markdown content request omitted owner authentication')
            expect(workspace.get_by_role('heading', name='Workspace acceptance', exact=True)).to_be_visible()
            expect(workspace.get_by_role('tab', name='workspace-review.md', exact=True)).to_be_visible()
            settle_workspace(workspace)
            page.screenshot(path=str(REPORT / 'workspace-markdown.png'), full_page=True)
            # Narrow layout uses the same native modal and keyboard focus owner.
            page.set_viewport_size({'width': 820, 'height': 900})
            page.wait_for_function('document.querySelector("#talos-right-workspace")?.matches(":modal")')
            require(workspace.evaluate('node => getComputedStyle(node, "::backdrop").backgroundColor') == 'rgba(0, 0, 0, 0)', 'Narrow workspace tinted the Computer rail')
            settle_workspace(workspace)
            page.screenshot(path=str(REPORT / 'workspace-narrow-markdown.png'), full_page=True)
            page.keyboard.press('Escape')
            expect(page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)).to_be_focused()
            page.get_by_role('button', name='Open sidebar', exact=True).click()
            sidebar = page.get_by_role('dialog', name='Projects and chats', exact=True)
            sidebar.get_by_role('button', name='Collapse sidebar', exact=True).focus()
            page.keyboard.press('Escape')
            expect(page.get_by_role('button', name='Open sidebar', exact=True)).to_be_focused()
            page.get_by_role('button', name='Open sidebar', exact=True).click()
            sidebar.get_by_role('button', name='Show workspace (2 open windows)', exact=True).click()
            expect(sidebar).to_have_count(0)
            expect(workspace.get_by_role('tab')).to_have_count(2)
            workspace.get_by_role('button', name='New webpage tab', exact=True).click()
            expect(workspace.get_by_role('tab')).to_have_count(3)
            workspace.get_by_role('button', name='Close New webpage tab', exact=True).click()
            page.keyboard.press('Escape')
            expect(page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)).to_be_focused()
            page.set_viewport_size({'width': 1560, 'height': 1000})
            page.get_by_role('button', name='Show workspace (2 open windows)', exact=True).click()
            # A real account logout is an actor scope transition; both resource
            # descriptors and hidden browsing contexts must be destroyed.
            page.get_by_role('button', name='alice', exact=True).click()
            page.get_by_role('button', name='Log out', exact=True).click()
            expect(page.get_by_role('heading', name='Signed out of Talos', exact=True)).to_be_visible()
            expect(page).to_have_url(re.compile('/auth/(login|logout)'))
            expect(page.locator('#talos-right-workspace')).to_have_count(0)
            require(not first.evaluate('(frame) => frame.isConnected'), 'Account scope change retained a private preview frame')
            require(not errors, 'Unexpected product runtime errors: ' + repr(errors))
            browser.close()
    finally:
        preview.shutdown()
        preview.server_close()
        preview_thread.join(timeout=5)
    print('CASE resource_workspace_real_product_tabs_state_keyboard_dialog_close_scope_markdown_auth PASS', flush=True)


if __name__ == '__main__':
    cluster = ContractAuthCluster()
    try:
        run(cluster)
    finally:
        cluster.close()
