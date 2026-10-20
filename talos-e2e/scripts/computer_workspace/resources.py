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
    context.add_init_script('if (window === window.top) localStorage.setItem("talos-web-ui", JSON.stringify({state:{locale:"en",uiTheme:"light"},version:0}));')


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
            expect(page.get_by_role('button', name='Your computer', exact=True)).to_have_count(0)
            page.get_by_role('button', name='New workspace tab', exact=True).click()
            workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
            expect(workspace).to_be_visible()
            expect(workspace.get_by_role('tab')).to_have_count(1)
            expect(workspace.get_by_role('tab')).to_be_focused()
            address = workspace.get_by_role('textbox', name='Web address', exact=True)
            address.fill(destination)
            address.press('Enter')
            first = workspace.locator('iframe').element_handle()
            first_frame = first.content_frame()
            first_frame.get_by_role('textbox', name='Draft').fill('first tab draft')
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
            workspace.get_by_role('button', name='Hide workspace (2 open windows)', exact=True).click()
            toggle = page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)
            expect(toggle).to_be_focused()
            require(first.evaluate('(frame) => frame.isConnected'), 'Collapsing destroyed a preview')
            toggle.click()
            expect(first_frame.get_by_role('textbox', name='Draft')).to_have_value('first tab draft')
            tabs.nth(0).press('End')
            expect(tabs.nth(1)).to_be_focused()
            expect(second_frame.get_by_role('textbox', name='Draft')).to_have_value('second tab draft')
            # Exercise the browser's real modal/top-layer/focus behavior, not jsdom.
            workspace.get_by_role('button', name='Expand workspace', exact=True).click()
            require(workspace.evaluate('(dialog) => dialog.matches(":modal")'), 'Expanded workspace is not a native modal dialog')
            page.keyboard.press('Escape')
            expect(workspace).not_to_be_visible()
            expect(page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)).to_be_focused()
            page.get_by_role('button', name='Show workspace (2 open windows)', exact=True).click()
            expect(workspace.get_by_role('button', name='Expand workspace', exact=True)).to_be_visible()
            require(not workspace.evaluate('(dialog) => dialog.matches(":modal")'), 'Reopening retained expanded modal state')
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
                page.get_by_role('button', name='Preview Markdown', exact=True).click()
            require(content_request.value.headers.get('authorization') == 'Bearer ' + cluster.users['alice']['access_token'], 'Product Markdown content request omitted owner authentication')
            expect(workspace.get_by_role('heading', name='Workspace acceptance', exact=True)).to_be_visible()
            expect(workspace.get_by_role('tab', name='workspace-review.md', exact=True)).to_be_visible()
            page.screenshot(path=str(REPORT / 'workspace-markdown.png'), full_page=True)
            # Narrow layout uses the same native modal and keyboard focus owner.
            page.set_viewport_size({'width': 820, 'height': 900})
            page.wait_for_function('document.querySelector("#talos-right-workspace")?.matches(":modal")')
            page.screenshot(path=str(REPORT / 'workspace-narrow-markdown.png'), full_page=True)
            page.keyboard.press('Escape')
            expect(page.get_by_role('button', name='Show workspace (2 open windows)', exact=True)).to_be_focused()
            page.get_by_role('button', name='Open sidebar', exact=True).click()
            sidebar = page.get_by_role('dialog', name='Projects and chats', exact=True)
            sidebar.get_by_role('button', name='New webpage tab', exact=True).click()
            expect(sidebar).to_have_count(0)
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
