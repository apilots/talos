"""Session-bound resource preview through real Web UI, Files, history and Turn APIs."""
import base64
import json
import io
import os
import re
import urllib.request
import zipfile
from preview_fixture import PreviewCluster
from fixture import ROOT, REPORT, data, free_port, require, wait_for
from resources import seed, settle_workspace
from playwright.sync_api import sync_playwright, expect

HTML = b'<!doctype html><title>Resource report</title><h1>Resource report</h1><label>Report draft<input aria-label="Report draft"></label>'
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=')


def upload(page, name, mime, body):
    with page.expect_response(lambda response: response.url.endswith('/v1/files') and response.request.method == 'POST') as uploaded:
        page.locator('input[type=file]').set_input_files({'name': name, 'mimeType': mime, 'buffer': body})
    return data(uploaded.value.json())['file']['fileId']


def run(cluster):
    cluster.start_control_plane()
    cluster.start_preview_worker()
    port = free_port()
    cluster.start_process('preview-web', ['pnpm', '--dir', str(ROOT / 'talos-app'), '--filter', 'talos-web', 'dev', '--host', '127.0.0.1', '--port', str(port), '--strictPort'], dict(os.environ, VITE_API_PROXY_TARGET=cluster.url))
    url = f'http://127.0.0.1:{port}'
    wait_for(lambda: urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=2).status == 200, 'Product Web UI')
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
        context = browser.new_context(viewport={'width': 1560, 'height': 1000}, locale='en-US', accept_downloads=True)
        seed(context, cluster.users['alice']['access_token'])
        page = context.new_page()
        errors = []
        page.on('pageerror', lambda error: errors.append(str(error)))
        try:
            page.goto(url + '/files')
            expect(page.get_by_role('button', name='Upload', exact=True)).to_be_visible(timeout=45_000)
            html = upload(page, 'report.html', 'text/html', HTML)
            image = upload(page, 'pixel.png', 'image/png', PNG)
            text = upload(page, 'data.csv', 'text/csv', b'name,value\npreview,true\n')
            archive_bytes = io.BytesIO()
            with zipfile.ZipFile(archive_bytes, 'w') as zipped:
                zipped.writestr(zipfile.ZipInfo('readme.txt', date_time=(2020, 1, 1, 0, 0, 0)), b'Preview download')
            archive = upload(page, 'archive.zip', 'application/zip', archive_bytes.getvalue())
            upload(page, 'notes.md', 'text/markdown', b'# Notes preview\n\nExisting **Markdown** rendering.')
            workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
            # Every Files item is a right-side entry, including images and unknown formats.
            page.get_by_role('button', name=re.compile('^pixel.png')).click()
            expect(workspace.get_by_role('tab', name='pixel.png', exact=True)).to_be_visible()
            expect(workspace.locator('iframe[title="pixel.png"]')).to_be_visible()
            expect(page.get_by_role('dialog', name='Preview pixel.png', exact=True)).to_have_count(0)
            page.get_by_role('button', name=re.compile('^data.csv')).click()
            expect(workspace.locator('pre:visible')).to_have_text('name,value\npreview,true\n')
            page.get_by_role('button', name=re.compile('^notes.md')).click()
            expect(workspace.get_by_role('heading', name='Notes preview')).to_be_visible()
            page.get_by_role('button', name=re.compile('^archive.zip')).click()
            expect(workspace.get_by_role('status')).to_contain_text('cannot be displayed')
            with page.expect_download() as downloaded:
                workspace.get_by_role('button', name='Download file', exact=True).click()
            require(downloaded.value.path().read_bytes() == archive_bytes.getvalue(), 'Authenticated download changed bytes')
            page.get_by_role('button', name=re.compile('^report.html')).click()
            library_frame = workspace.locator('iframe[title="report.html"]').element_handle()
            library_draft = library_frame.content_frame().get_by_role('textbox', name='Report draft')
            library_draft.fill('Files-only draft')
            # Radix menu must stay inside the native top-layer dialog on narrow screens.
            page.set_viewport_size({'width': 820, 'height': 900})
            settle_workspace(workspace)
            workspace.get_by_role('button', name='Open in options', exact=True).last.click()
            expect(page.get_by_role('menuitem', name='Talos', exact=True)).to_be_visible()
            expect(page.get_by_role('menuitem', name='Copy link', exact=True)).to_be_visible()
            page.screenshot(path=str(REPORT / 'preview-narrow-menu.png'), full_page=True)
            page.keyboard.press('Escape')
            page.keyboard.press('Escape')
            page.reload()
            expect(page.get_by_role('button', name='Show workspace (5 open windows)', exact=True)).to_be_visible(timeout=45_000)
            expect(workspace).not_to_be_visible()
            page.set_viewport_size({'width': 1560, 'height': 1000})
            page.get_by_role('button', name='Show workspace (5 open windows)', exact=True).click()
            # Real stored sessions/history are produced by a controlled model upstream, never seeded UI state.
            destination = f'http://127.0.0.1:{cluster.upstream.server_port}/'
            a = cluster.session(f'Preview session A\n\n[Preview report](talos-file://{html})\n\n[Local website]({destination})', (image, text))
            b = cluster.session('Preview session B')
            page.goto(url + '/sessions/' + a)
            expect(page.get_by_role('link', name='Preview report', exact=True)).to_be_visible(timeout=45_000)
            expect(workspace).not_to_be_visible()
            page.get_by_role('button', name='Preview pixel.png', exact=True).click()
            lightbox = page.get_by_role('dialog', name='Preview pixel.png', exact=True)
            expect(lightbox).to_be_visible()
            require(page.locator('#talos-right-workspace [role=tab]').count() == 0, 'History image created a workspace tab')
            page.keyboard.press('Escape')
            page.get_by_role('button', name='data.csv', exact=True).click()
            expect(workspace.locator('pre:visible')).to_have_text('name,value\npreview,true\n')
            page.get_by_role('link', name='Preview report', exact=True).click()
            session_frame = workspace.locator('iframe[title="report.html"]').last.element_handle()
            draft = session_frame.content_frame().get_by_role('textbox', name='Report draft')
            draft.fill('Session A draft')
            require(session_frame.content_frame().evaluate('typeof window.talos') == 'undefined', 'HTML inherited host bridge')
            page.get_by_text('Preview session B', exact=True).first.click()
            expect(workspace).not_to_be_visible()
            page.get_by_role('button', name='New workspace tab', exact=True).click()
            expect(workspace.get_by_role('tab')).to_have_count(1)
            page.get_by_text('Preview session A', exact=True).first.click()
            expect(workspace.get_by_role('tab')).to_have_count(2)
            expect(draft).to_have_value('Session A draft')
            require(session_frame.evaluate('frame => frame.isConnected'), 'Session round-trip replaced HTML instance')
            # Browser history changes the owner without an outside click dismissing its menu.
            workspace.get_by_role('button', name='Open in options', exact=True).last.click()
            expect(page.get_by_role('menuitem', name='Talos', exact=True)).to_be_visible()
            page.go_back()
            expect(page).to_have_url(url + '/sessions/' + b)
            expect(workspace.get_by_role('tab')).to_have_count(1)
            expect(page.get_by_role('menu')).to_have_count(0)
            page.go_forward()
            expect(page).to_have_url(url + '/sessions/' + a)
            expect(draft).to_have_value('Session A draft')
            page.get_by_role('link', name='Local website', exact=True).click()
            expect(workspace.get_by_role('tab')).to_have_count(3)
            expect(workspace.get_by_role('textbox', name='Web address', exact=True)).to_have_value(destination)
            page.get_by_role('link', name='Files', exact=True).click()
            expect(workspace.get_by_role('tab')).to_have_count(5)
            expect(workspace.get_by_role('tab', name='report.html')).to_have_attribute('aria-selected', 'true')
            library_frame = workspace.locator('iframe[title="report.html"]:visible').element_handle()
            library_draft = library_frame.content_frame().get_by_role('textbox', name='Report draft')
            library_draft.fill('Retained library draft')
            settle_workspace(workspace)
            page.screenshot(path=str(REPORT / 'preview-files-html.png'), full_page=True)
            page.get_by_text('Preview session A', exact=True).first.click()
            expect(page).to_have_url(url + '/sessions/' + a)
            expect(workspace.get_by_role('tab')).to_have_count(3)
            workspace.get_by_role('tab', name='report.html').click()
            expect(draft).to_have_value('Session A draft')
            workspace.get_by_role('button', name='Refresh webpage', exact=True).click()
            expect(workspace.frame_locator('iframe[title="report.html"]:visible').get_by_role('textbox', name='Report draft')).to_have_value('')
            page.screenshot(path=str(REPORT / 'preview-session-restored.png'), full_page=True)
            page.get_by_role('link', name='Files', exact=True).click()
            expect(workspace.get_by_role('tab')).to_have_count(5)
            expect(library_draft).to_have_value('Retained library draft')
            page.screenshot(path=str(REPORT / 'preview-files-retained.png'), full_page=True)
            require(not errors, 'Product runtime errors: ' + repr(errors))
            (REPORT / 'resource-preview.json').write_text(json.dumps({'status': 'passed', 'sessions': [a, b], 'fileIds': [html, image, text, archive], 'frontendMocks': False, 'upstreams': ['contract-auth', 'controlled-chat-model']}, indent=2) + '\n')
        except Exception:
            page.screenshot(path=str(REPORT / 'resource-preview-failure.png'), full_page=True)
            raise
        finally:
            browser.close()
    print('CASE unified_preview_web_files_session_images_scopes_refresh_menu_download PASS', flush=True)


if __name__ == '__main__':
    cluster = PreviewCluster()
    try:
        run(cluster)
    finally:
        cluster.close()
