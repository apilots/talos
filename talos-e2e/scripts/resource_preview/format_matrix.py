"""One real format matrix for the shared Web/Desktop preview implementation."""
import json
import gzip
from pathlib import Path
import subprocess
import base64
import shutil
from urllib.parse import urlencode
from urllib.parse import unquote, urlsplit

from fixture import ROOT, REPORT, data, require, wait_for, http
from resources import settle_workspace
from playwright.sync_api import expect

SOURCES = ROOT / 'talos-e2e/fixtures/resource-preview'
CASES = json.loads((SOURCES / 'manifest.json').read_text())
FIXTURES = REPORT / 'format-fixtures'
FIXTURES.mkdir(parents=True, exist_ok=True)
for case in CASES:
    source = SOURCES / case.get('fixture', case['name'])
    content = source.read_bytes()
    (FIXTURES / case['name']).write_bytes(gzip.decompress(content) if source.suffix == '.gz' else content)


def verify_default_admission(cluster):
    """Exercise the real default FileService policy separately from opt-in UI tests."""
    cluster.start_control_plane()
    results = []
    for case in [row for row in CASES if row['admission'] == 'default'] + [row for row in CASES if row['name'] in ('matrix-content.html', 'matrix-bundle.zip')]:
        metadata = json.dumps({'filename': case['name'], 'scope': {'type': 'personal'}, 'tags': [], 'metadata': {}})
        response = subprocess.run(['curl', '--fail-with-body', '-sS', '--noproxy', '*', cluster.url + '/v1/files',
            '-H', 'Authorization: Bearer ' + cluster.users['alice']['access_token'],
            '-F', 'metadata=' + metadata + ';type=application/json',
            '-F', 'file=@' + str(FIXTURES / case['name']) + ';type=application/octet-stream'], capture_output=True)
        body = json.loads(response.stdout)
        if case['admission'] == 'default':
            file = data(body)['file']
            require(file['mimeType'] == case['mime'], 'Default admission MIME differed: ' + repr(file))
            result = 'accepted'
        else:
            require(body.get('error', {}).get('code') == 'unsupportedMediaType', 'Preview capability bypassed default Files admission: ' + repr(body))
            result = 'rejected'
        results.append({'name': case['name'], 'expectedMime': case['mime'], 'result': result})
    (REPORT / 'default-files-admission.json').write_text(json.dumps({'status': 'passed', 'configuredOptIns': [], 'cases': results}, indent=2) + '\n')


def exercise_formats(page, upload, *, native_pages=None, cases=CASES):
    """Upload through the shell's real Files UI, then assert decoded/rendered content.

    upload(case) returns server metadata after actual upload; the shared exercise
    owns all format assertions. Native pages differ only in surface placement.
    """
    workspace = page.get_by_role('dialog', name='Right workspace', exact=True)
    folder = REPORT / 'format-matrix'; folder.mkdir(exist_ok=True)
    results = []
    for case in cases:
        result = dict(case, shell='desktop' if native_pages else 'web', source='Files')
        try:
            file = upload(case)
            result['detectedMime'] = file['mimeType']
            require(file['mimeType'] == case['mime'], 'FileService detected a different MIME: ' + repr(file))
            expect(page.get_by_role('heading', name=case['name'], exact=True)).to_be_visible()
            page.get_by_role('button', name='Open file', exact=True).click()
            settle_workspace(workspace)
            viewer = case['viewer']
            if viewer in ('image', 'html', 'audio', 'video'):
                if native_pages:
                    surface = wait_for(lambda: next((child for child in native_pages(page)
                        if child.url.startswith('talos-preview:') and unquote(urlsplit(child.url).path).lstrip('/') == case['name']), None), 'Native format ' + case['name'])
                else:
                    surface = workspace.frame_locator('iframe[title=' + json.dumps(case['name']) + ']:visible')
                if viewer == 'image':
                    image = surface.locator('img').first
                    wait_for(lambda: image.evaluate('(image) => image.complete && image.naturalWidth > 0'), 'Decoded image ' + case['name'])
                    require(image.evaluate('(image) => [image.naturalWidth,image.naturalHeight]') == [case['width'], case['height']], 'Decoded image dimensions differed')
                    result['assertion'] = 'decoded image dimensions'
                elif viewer == 'html':
                    expect(surface.get_by_role('heading', name=case['text'], exact=True)).to_be_visible()
                    surface.get_by_role('textbox', name='Format draft').fill('Format matrix draft')
                    result['assertion'] = 'document content and form'
                else:
                    media = surface.locator('video,audio').first
                    wait_for(lambda: media.evaluate('(media) => media.readyState >= 2 && Number.isFinite(media.duration) && media.duration > 0'), 'Decoded media ' + case['name'])
                    if viewer == 'video':
                        require(media.evaluate('(media) => [media.videoWidth,media.videoHeight]') == [case['width'], case['height']], 'Video dimensions differed')
                    media.evaluate('async (media) => { media.muted = true; await media.play(); }')
                    wait_for(lambda: media.evaluate('(media) => media.currentTime > 0'), 'Media playback ' + case['name'])
                    result['duration'] = media.evaluate('(media) => media.duration')
                    media.evaluate('(media) => media.pause()')
                    result['assertion'] = 'decoded media metadata and advancing playback'
                if viewer == 'html':
                    workspace.get_by_role('button', name='Source', exact=True).click()
                    expect(workspace.locator('.cm-content:visible')).to_contain_text(case['text'])
                    source_capture = 'format-matrix/' + case['name'] + '.source.png'
                    page.screenshot(path=str(REPORT / source_capture), full_page=True)
                    result['sourceScreenshot'] = source_capture
                    workspace.get_by_role('button', name='Preview', exact=True).click()
                    if native_pages:
                        surface = wait_for(lambda: next((child for child in native_pages(page)
                            if child.url.startswith('talos-preview:') and unquote(urlsplit(child.url).path).lstrip('/') == case['name']), None), 'Restored native HTML')
                    expect(surface.get_by_role('heading', name=case['text'], exact=True)).to_be_visible()
                    result['assertion'] += ', same-tab HTML source/preview roundtrip'
                if native_pages:
                    surface.screenshot(path=str(folder / (case['name'] + '.content.png')))
                    result['contentScreenshot'] = 'format-matrix/' + case['name'] + '.content.png'
            elif viewer == 'pdf':
                assert_pdf(workspace, case)
                result['firstPageScreenshot'] = 'format-matrix/' + case['name'] + '.page-1.png'
                result['assertion'] = 'actual PDF canvas, first-page text and second-page text' + (' / Chinese text' if case.get('chineseText') else '')
            elif viewer == 'unsupported':
                expect(workspace.get_by_role('heading', name=case['name'], exact=True)).to_be_visible()
                expect(workspace.get_by_role('status')).to_contain_text('Download')
                expect(workspace.locator('p:visible').filter(has_text=case['mime'])).to_be_visible()
                with page.expect_download() as downloaded:
                    workspace.get_by_role('button', name='Download original file', exact=True).click()
                require(downloaded.value.path().read_bytes() == (FIXTURES / case['name']).read_bytes(), 'Package download changed original bytes')
                result['assertion'] = 'package metadata and byte-exact original download; no member preview'
                if case['name'].endswith('.tar'):
                    original = page.viewport_size or page.evaluate('({width:innerWidth,height:innerHeight})')
                    page.set_viewport_size({'width': 980 if native_pages else 820, 'height': original['height']})
                    settle_workspace(workspace)
                    expect(workspace.get_by_role('heading', name=case['name'], exact=True)).to_be_visible()
                    narrow = 'format-matrix/' + case['name'] + '.narrow.png'
                    page.screenshot(path=str(REPORT / narrow), full_page=True);result['narrowScreenshot'] = narrow
                    workspace.get_by_role('button', name='Open in options', exact=True).click()
                    expect(workspace.get_by_role('menuitem', name='Talos', exact=True)).to_be_visible()
                    menu = 'format-matrix/' + case['name'] + '.menu.png'
                    page.screenshot(path=str(REPORT / menu), full_page=True);result['menuScreenshot'] = menu
                    page.keyboard.press('Escape')
                    page.set_viewport_size(original);settle_workspace(workspace)
            elif viewer == 'markdown':
                expect(workspace.get_by_role('heading', name=case['text'], exact=True)).to_be_visible()
                expect(workspace.get_by_role('table')).to_be_visible()
                result['assertion'] = 'rendered heading, emphasis and table'
            else:
                expect(workspace.locator('.cm-content:visible')).to_contain_text(case['text'].replace('\n', ''))
                result['assertion'] = 'decoded source text'
            expect(workspace.get_by_role('alert')).to_have_count(0)
            tab = workspace.get_by_role('tab', selected=True)
            expect(tab).to_have_accessible_name(case['name'])
            def selected_tab_visible():
                current = tab.bounding_box(); strip = workspace.get_by_role('tablist').bounding_box()
                return current and strip and current['x'] >= strip['x'] - 1 and current['x'] + current['width'] <= strip['x'] + strip['width'] + 1
            wait_for(selected_tab_visible, 'Selected filename remains visible in the tab strip')
            wait_for(lambda: abs(workspace.bounding_box()['x'] + workspace.bounding_box()['width'] - page.evaluate('innerWidth')) <= 1, 'Workspace stays anchored to the right after passive resize')
            result['activeTab'] = tab.inner_text()
            result['location'] = workspace.get_by_role('textbox', name='File location').input_value()
            result['status'] = 'passed'
        except Exception as error:
            result.update(status='failed', error=str(error))
        finally:
            screenshot = 'format-matrix/' + case['name'] + '.png'
            page.screenshot(path=str(REPORT / screenshot), full_page=True)
            result['screenshot'] = screenshot
            results.append(result)
            (REPORT / 'format-matrix.json').write_text(json.dumps(results, indent=2, ensure_ascii=False) + '\n')
            close = workspace.get_by_role('button', name='Close ' + case['name'], exact=True)
            if close.count(): close.click()
    require(all(result['status'] == 'passed' for result in results), 'Format matrix failures: ' + repr([(r['name'], r.get('error')) for r in results if r['status'] != 'passed']))
    return results


def assert_pdf(workspace, case, *, evidence_name=None):
    expect(workspace.locator('.react-pdf__Page canvas:visible')).to_be_visible(timeout=90_000)
    expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text(case['text'])
    if case.get('chineseText'):
        expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text(case['chineseText'])
    if case.get('detail'):
        expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text(case['detail'])
    (REPORT / 'format-matrix').mkdir(exist_ok=True)
    workspace.page.screenshot(path=str(REPORT / ('format-matrix/' + (evidence_name or case['name']) + '.page-1.png')), full_page=True)
    workspace.get_by_role('button', name='Next page', exact=True).click()
    expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text(case['secondText'])
    if case.get('chineseText') and 'slides' in case['name']:
        expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text(case['chineseText'])


def verify_session_sources(cluster):
    """Same fixtures enter via saved Worker paths, without Files upload/admission."""
    token = cluster.users['alice']['access_token']
    node = cluster.nodes['preview']['node_id']
    info = data(http(cluster.url + '/v1/workspace/info?' + urlencode({'nodeId': node}), token=token))
    root = Path(info['userWorkspaceDir']) / 'formats'; root.mkdir(parents=True, exist_ok=True)
    for case in CASES: shutil.copyfile(FIXTURES / case['name'], root / case['name'])
    session = cluster.session('Session format preview\n\n[Session presentation](formats/matrix-slides.pptx)')
    results = []
    for case in CASES:
        grant = data(http(cluster.url + '/v1/file-previews', {'source': {'kind': 'sessionPath', 'sessionId': session, 'path': 'formats/' + case['name']}}, token))
        try:
            expected = 'image/svg+xml' if case['name'].endswith('.svg') else case['mime']
            require(grant['metadata']['mimeType'] == expected, 'Worker metadata differs for ' + case['name'] + ': ' + repr(grant['metadata']))
            pdf = case['viewer'] == 'pdf' and case['mime'] != 'application/pdf'
            if pdf:
                require(grant['document']['kind'] == 'pdf', 'Session Office conversion failed for ' + case['name'] + ': ' + repr(grant['document']))
            path = grant['document']['resourcePath'] if pdf else grant['entryPath']
            part = data(http(cluster.url + '/v1/file-previews/read', {'previewId': grant['previewId'], 'resourcePath': path, 'offset': 0, 'limit': 64}, token))
            expected_bytes = b'%PDF' if pdf else (FIXTURES / case['name']).read_bytes()[:64]
            require(base64.b64decode(part['dataBase64']).startswith(expected_bytes), 'Session preview bytes differ for ' + case['name'])
            results.append({'name': case['name'], 'source': 'sessionPath', 'detectedMime': grant['metadata']['mimeType'], 'assertion': 'prepare/read + document capability (API)', 'status': 'passed'})
            (REPORT / 'session-format-sources.json').write_text(json.dumps(results, indent=2) + '\n')
        finally:
            data(http(cluster.url + '/v1/file-previews/close', {'previewId': grant['previewId']}, token))
    return session
