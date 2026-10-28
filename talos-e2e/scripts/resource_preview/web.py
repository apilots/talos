"""Session-bound resource preview through real Web UI, Files, history and Turn APIs."""
import base64
import json
import io
import os
import re
import subprocess
import urllib.request
import zipfile
from preview_fixture import PreviewCluster
from fixture import ROOT, REPORT, data, free_port, require, wait_for, http
from urllib.parse import urlencode
from document_fixture import office_document, workspace_files
from format_matrix import CASES, FIXTURES, exercise_formats, verify_default_admission, verify_session_sources, assert_pdf
from resources import seed, settle_workspace
from playwright.sync_api import sync_playwright, expect

HTML = b'<!doctype html><title>Resource report</title><h1>Resource report</h1><label>Report draft<input aria-label="Report draft"></label>'
PNG = base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=')


def upload(page, name, mime, body, expected_mime=None):
    expect(page.get_by_role("button", name="Upload", exact=True)).to_be_visible()
    with page.expect_response(lambda response: response.url.endswith('/v1/files') and response.request.method == 'POST') as uploaded:
        page.locator('input[type=file]').set_input_files({'name': name, 'mimeType': mime, 'buffer': body})
    file = data(uploaded.value.json())['file']
    if expected_mime is not None:
        require(file['mimeType'] == expected_mime, 'Server MIME did not reflect actual uploaded content')
    return file['fileId']


def start_web_ui(cluster):
    port = free_port()
    renderer = cluster.root / 'web-renderer'
    env = dict(os.environ, VITE_API_PROXY_TARGET=cluster.url, TALOS_WEB_BASE='/', VITE_TALOS_DESKTOP='0')
    with (REPORT / 'web-preview-build.log').open('wb') as output:
        subprocess.run(['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', 'build', '--outDir', str(renderer)], env=env, stdout=output, stderr=subprocess.STDOUT, check=True, timeout=180)
    cluster.start_process('preview-web', ['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', 'preview', '--outDir', str(renderer), '--host', '127.0.0.1', '--port', str(port), '--strictPort'], env)
    url = f'http://127.0.0.1:{port}'
    wait_for(lambda: urllib.request.build_opener(urllib.request.ProxyHandler({})).open(url, timeout=2).status == 200, 'Product Web UI')
    return url


def run(cluster):
    cluster.start_control_plane()
    cluster.start_preview_worker()
    url = start_web_ui(cluster)
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN', '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
        context = browser.new_context(viewport={'width': 1560, 'height': 1000}, locale='en-US', accept_downloads=True)
        seed(context, cluster.users['alice']['access_token'])
        page = context.new_page()
        errors, console_errors, requests, lifecycle = [], [], [], []
        page.on("framedetached", lambda frame: lifecycle.append({"event": "detached", "url": frame.url}))
        page.on("framenavigated", lambda frame: lifecycle.append({"event": "navigated", "url": frame.url}))
        page.on("request", lambda request: lifecycle.append({"event": "preview-request", "page": page.url, "url": request.url, "body": request.post_data_json}) if "file-previews" in request.url and request.method == "POST" else None)
        page.on("console", lambda message: console_errors.append(message.text) if message.type == "error" else None)
        page.on("response", lambda response: requests.append({"url": response.url, "status": response.status}) if ".preview.localhost" in response.url else None)
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
            generated_json = b'{"preview":true,"source":"stored JSON"}'
            generated_code = b'export const preview = "stored source";\n'
            upload(page, 'result.json', 'application/json', generated_json, expected_mime='application/json')
            upload(page, 'module.ts', 'application/octet-stream', generated_code, expected_mime='text/plain')
            workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
            page.get_by_role('button', name=re.compile('^result.json')).click()
            expect(workspace.locator('.cm-content:visible')).to_have_text(generated_json.decode())
            page.screenshot(path=str(REPORT / 'preview-generated-json.png'), full_page=True)
            page.get_by_role('button', name=re.compile('^module.ts')).click()
            expect(workspace.locator('.cm-content:visible')).to_have_text(generated_code.decode())
            page.screenshot(path=str(REPORT / 'preview-generated-source.png'), full_page=True)
            # Every Files item is a right-side entry, including images and unknown formats.
            page.get_by_role('button', name=re.compile('^pixel.png')).click()
            expect(workspace.get_by_role('tab', name='pixel.png', exact=True)).to_be_visible()
            expect(workspace.locator('iframe[title="pixel.png"]')).to_be_visible()
            expect(page.get_by_role('dialog', name='Preview pixel.png', exact=True)).to_have_count(0)
            page.get_by_role('button', name=re.compile('^data.csv')).click()
            expect(workspace.locator('.cm-content:visible')).to_contain_text('preview,true')
            page.get_by_role('button', name=re.compile('^notes.md')).click()
            expect(workspace.get_by_role('heading', name='Notes preview')).to_be_visible()
            page.get_by_role('button', name=re.compile('^archive.zip')).click()
            expect(workspace.get_by_role('button', name=re.compile('^readme.txt'))).to_be_visible()
            with page.expect_download() as downloaded:
                workspace.get_by_role('button', name='Download file', exact=True).click()
            require(downloaded.value.path().read_bytes() == archive_bytes.getvalue(), 'Authenticated download changed bytes')
            workspace.get_by_role('button', name=re.compile('^readme.txt')).click()
            expect(workspace.locator('.cm-content:visible')).to_have_text('Preview download')
            workspace.get_by_role('button', name='Back to archive', exact=True).click()
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
            expect(page.get_by_role('button', name='Show workspace (7 open windows)', exact=True)).to_be_visible(timeout=45_000)
            expect(workspace).not_to_be_visible()
            page.set_viewport_size({'width': 1560, 'height': 1000})
            page.get_by_role('button', name='Show workspace (7 open windows)', exact=True).click()
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
            expect(workspace.locator('.cm-content:visible')).to_contain_text('preview,true')
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
            expect(workspace.get_by_role('tab')).to_have_count(7)
            expect(workspace.get_by_role('tab', name='report.html')).to_have_attribute('aria-selected', 'true')
            settle_workspace(workspace)
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
            expect(workspace.get_by_role('tab')).to_have_count(7)
            expect(library_draft).to_have_value('Retained library draft')
            page.screenshot(path=str(REPORT / 'preview-files-retained.png'), full_page=True)
            # A real remote execution Worker serves file bytes/assets through the common grant.
            node = cluster.nodes['preview']['node_id']
            info = data(http(cluster.url + '/v1/workspace/info?' + urlencode({'nodeId': node}), token=cluster.users['alice']['access_token']))
            workspace_files(info['userWorkspaceDir'])
            remote = cluster.session('Worker file preview\n\n[Worker report](reports/report.html)\n\n[Large source](./large.txt:54000)\n\n[Encoded source](encoded.txt)')
            page.goto(url + '/sessions/' + remote)
            page.get_by_role('link', name='Worker report', exact=True).click()
            report = workspace.frame_locator('iframe[title="report.html"]:visible')
            expect(report.get_by_role('heading', name='Worker file report')).to_have_css('color', 'rgb(17, 102, 51)')
            expect(report.locator('#script')).to_have_text('Worker script loaded')
            expect(report.get_by_alt_text('Worker asset')).to_be_visible()
            require(report.get_by_alt_text('Worker asset').evaluate('img => img.complete && img.naturalWidth === 20'), 'Root-relative Worker image did not load')
            settle_workspace(workspace)
            report.get_by_role('link', name='Next file').click()
            expect(report.get_by_role('heading', name='Worker next page')).to_be_visible()
            report.get_by_role('link', name='Back to report').click()
            expect(report.get_by_role('heading', name='Worker file report')).to_be_visible()
            page.screenshot(path=str(REPORT / 'worker-html-assets.png'), full_page=True)
            page.get_by_role('link', name='Large source', exact=True).click()
            expect(workspace.get_by_role('textbox', name='Line', exact=True)).to_have_value('54000')
            expect(workspace.locator('.cm-content:visible')).to_contain_text('054000 Worker source line')
            page.get_by_role('link', name='Encoded source', exact=True).click()
            workspace.get_by_role('combobox', name='Encoding', exact=True).select_option('utf-16le')
            expect(workspace.locator('.cm-content:visible')).to_have_text('第一行第二行')
            page.get_by_role('link', name='Files', exact=True).click()
            upload(page, 'office.docx', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document', office_document())
            page.get_by_role('button', name=re.compile('^office.docx')).click()
            expect(workspace.locator('.react-pdf__Page canvas')).to_be_visible(timeout=90_000)
            expect(workspace.get_by_text('Office preview first page', exact=True)).to_be_visible()
            workspace.get_by_role('button', name='Next page', exact=True).click()
            expect(workspace.get_by_text('Office preview second page', exact=True)).to_be_visible()
            page.screenshot(path=str(REPORT / 'office-pdf-render.png'), full_page=True)
            page.reload()
            expect(workspace.get_by_text('Office preview second page', exact=True)).to_be_visible(timeout=90_000)
            # Existing outbound transport carries the same source contract without a source reader on Server.
            cluster.start_preview_worker('outbound-preview', 'outbound')
            outbound = cluster.nodes['outbound-preview']['node_id']
            info = data(http(cluster.url + '/v1/workspace/info?' + urlencode({'nodeId': outbound}), token=cluster.users['alice']['access_token']))
            root = workspace_files(info['userWorkspaceDir'])
            outbound_session = cluster.session('Outbound file acceptance', worker='outbound-preview')
            grant = data(http(cluster.url + '/v1/file-previews', {'source': {'kind': 'sessionPath', 'sessionId': outbound_session, 'path': 'reports/report.html'}}, cluster.users['alice']['access_token']))
            part = data(http(cluster.url + '/v1/file-previews/read', {'previewId': grant['previewId'], 'resourcePath': grant['entryPath'], 'offset': 0, 'limit': 4096}, cluster.users['alice']['access_token']))
            require(base64.b64decode(part['dataBase64']) == (root / 'reports/report.html').read_bytes(), 'Outbound Worker file bytes changed')
            data(http(cluster.url + '/v1/file-previews/close', {'previewId': grant['previewId']}, cluster.users['alice']['access_token']))
            def upload_format(case):
                file_id = upload(page, case['name'], 'application/octet-stream', (FIXTURES / case['name']).read_bytes(), expected_mime=case['mime'])
                return {'fileId': file_id, 'mimeType': case['mime']}
            exercise_formats(page, upload_format)
            session_formats = verify_session_sources(cluster)
            page.goto(url + '/sessions/' + session_formats)
            page.get_by_role('link', name='Session presentation', exact=True).click()
            assert_pdf(workspace, next(case for case in CASES if case['name'] == 'matrix-slides.pptx'))
            page.screenshot(path=str(REPORT / 'session-presentation.png'), full_page=True)
            require(not errors, 'Product runtime errors: ' + repr(errors))
            (REPORT / 'resource-preview.json').write_text(json.dumps({'status': 'passed', 'sessions': [a, b], 'fileIds': [html, image, text, archive], 'frontendMocks': False, 'upstreams': ['contract-auth', 'controlled-chat-model']}, indent=2) + '\n')
        except Exception:
            (REPORT / 'preview-diagnostics.json').write_text(json.dumps({'console': console_errors, 'resources': requests, 'lifecycle': lifecycle, 'workspace': page.evaluate('localStorage.getItem("talos-web-ui")'), 'frames': [{'url': frame.url, 'title': frame.title()} for frame in page.frames]}, indent=2))
            page.screenshot(path=str(REPORT / 'resource-preview-failure.png'), full_page=True)
            raise
        finally:
            browser.close()
    print('CASE unified_preview_web_files_session_images_scopes_refresh_menu_download PASS', flush=True)


if __name__ == '__main__':
    default_cluster = PreviewCluster(format_opt_ins=False)
    try:
        verify_default_admission(default_cluster)
    finally:
        default_cluster.close()
    cluster = PreviewCluster()
    try:
        run(cluster)
    finally:
        cluster.close()
