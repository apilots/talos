#!/usr/bin/env python3
"""Open the real Electron shell and verify Server-origin Talo media."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import uuid
from urllib.parse import urlsplit

from playwright.sync_api import Error, expect, sync_playwright

ROOT = Path(__file__).resolve().parents[3]
DESKTOP = ROOT / 'talos-app/desktop'
sys.path.insert(0, str(ROOT / 'talos-e2e/scripts'))
from lifecycle import stop_group


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def electron():
    default = DESKTOP / 'node_modules/electron/dist'
    native = (default / 'Electron.app/Contents/MacOS/Electron' if sys.platform == 'darwin'
              else default / 'electron')
    return Path(os.environ.get('TALOS_E2E_ELECTRON', native)).resolve(strict=True)


def wait_for_pixels(page):
    page.wait_for_function('''() => [...document.querySelectorAll('canvas')].some(canvas => {
        if (canvas.width < 20 || canvas.height < 20) return false;
        const context = canvas.getContext('2d');
        if (!context) return false;
        const colors = new Set();
        for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) {
            colors.add(String(context.getImageData(Math.floor(canvas.width*x/8), Math.floor(canvas.height*y/8), 1, 1).data));
        }
        return colors.size > 3;
    })''', polling=100, timeout=24000)


def sign_in(page):
    button = page.get_by_role('button', name='Continue with Talos Cloud')
    button.click(timeout=18000)
    button.wait_for(state='hidden', timeout=38000)


def verify_web_history(playwright, base, session, prompt, report):
    browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN') or
        '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless=True)
    page = browser.new_page(locale='en-US')
    try:
        page.goto(base)
        sign_in(page)
        page.goto(base + '/sessions/' + session)
        expect(page.get_by_text(prompt, exact=True)).to_be_visible(timeout=30000)
        expect(page.get_by_text('TALO_CHAT_OK', exact=True)).to_be_visible()
        page.screenshot(path=report / 'web-native-chat-history.png')
        print('PASS Web reopened the Server conversation and displayed the Desktop message and reply', flush=True)
    except Exception:
        page.screenshot(path=report / 'web-native-chat-history-failure.png', timeout=3000)
        raise
    finally:
        browser.close()


def submit_automation(page, base, stage, report):
    from managed_talo import ASSISTANT, data, http, native_automation_prompt, wait

    instance = data(http(base + ASSISTANT))['instance']
    session = instance['conversationSessionId']
    # Exercise activation on a ready Worker, including the direct link with a fresh native profile.
    data(http(base + f"/v1/fs/directory?nodeId={instance['nodeId']}&wakeIfSleeping=true"))
    page.get_by_role('button', name='Add', exact=True).click()
    page.get_by_role('button', name=stage.title(), exact=True).click()
    expect(page.get_by_test_id('automation-mode-chip')).to_contain_text(stage.title())
    page.get_by_role('textbox').last.fill(native_automation_prompt(stage))
    assert data(http(base + ASSISTANT))['instance']['phase'] == 'ready'
    with page.expect_request('talos-cloud://server/v1/turns/start'):
        page.get_by_role('button', name='Send', exact=True).click()

    def registered():
        current = data(http(base + '/v1/automation/read', {'sessionId': session}))
        return current['goal'] if stage == 'goal' else next(iter(current['loops']), None)

    task = wait(registered, f'native {stage} registered on Server')
    row = page.get_by_test_id('automation-task-' + task['id'])
    expect(row).to_contain_text('AUTO_CASE=', timeout=15000)
    page.screenshot(path=report / f'electron-native-{stage}.png')


def capture(playwright, process, endpoint, base, node_id, report, stage):
    deadline = time.monotonic() + 15
    while True:
        try:
            browser = playwright.chromium.connect_over_cdp(endpoint, timeout=1200)
            break
        except Error:
            if process.poll() is not None or time.monotonic() >= deadline:
                raise
            time.sleep(.2)
    context = browser.contexts[0]
    page = context.pages[0] if context.pages else context.wait_for_event('page', timeout=15000)
    if stage in ('initialize', 'chat', 'goal', 'loop'):
        page.wait_for_function("location.protocol === 'file:' && typeof window.talos?.invoke === 'function'")
        try:
            sys.path[:0] = [str(ROOT / 'talos-e2e/scripts/browser-worker'), str(ROOT / 'talos-e2e/scripts')]
            from managed_talo import ASSISTANT, data, http, managed_resource, wait
            if stage == 'loop':
                session = data(http(base + ASSISTANT))['instance']['conversationSessionId']
                page.goto(page.url.split('#')[0] + '#/sessions/' + session)
                page.get_by_role('tab', name='Conversation').wait_for()
                expect(page.get_by_text('Goal completion declared; settle this Turn normally', exact=True)).to_be_visible(timeout=30000)
            else:
                chooser = page.get_by_role('tab', name='Talo', exact=True)
                chooser.or_(page.get_by_role('link', name='Talo', exact=True)).first.click()
            if stage in ('goal', 'loop'):
                submit_automation(page, base, stage, report)
                return
            if stage == 'chat':
                talo_node = data(http(base + ASSISTANT))['instance']['nodeId']
                selection = "() => Object.values(JSON.parse(localStorage.getItem('talos-web-ui')).state.selectedComputers)"
                assert talo_node not in page.evaluate(selection)
                page.get_by_text('TALO_WAKE_OK', exact=True).wait_for(timeout=30000)
                attachment = 'native-talo.png'
                image = page.screenshot(clip={'x': 0, 'y': 0, 'width': 8, 'height': 8})
                page.locator('input[type=file]').set_input_files({'name': attachment, 'mimeType': 'image/png', 'buffer': image})
                expect(page.get_by_role('button', name='Send', exact=True)).to_be_enabled(timeout=15000)
                page.get_by_role('button', name='Remove ' + attachment, exact=True).click()
                page.get_by_role('button', name='Add', exact=True).click()
                page.get_by_role('button', name='Choose an existing file').click()
                page.get_by_role('button', name=attachment, exact=True).click()
                document = ROOT / 'talos-e2e/fixtures/resource-preview/matrix-word.pdf'
                page.locator('input[type=file]').set_input_files(str(document))
                expect(page.get_by_role('button', name='Send', exact=True)).to_be_enabled(timeout=15000)
                session_id = data(http(base + ASSISTANT))['instance']['conversationSessionId']
                prompt = 'native chat verification'
                page.get_by_role('textbox').last.fill(prompt)
                with page.expect_request('talos-cloud://server/v1/turns/start') as sent:
                    page.get_by_role('button', name='Send', exact=True).click()
                assert sent.value.post_data_json['sessionId'] == session_id
                page.get_by_text('TALO_CHAT', exact=True).wait_for(timeout=30000)
                Path(os.environ['TALOS_E2E_CHAT_VISIBLE']).touch()
                reply = page.get_by_text('TALO_CHAT_OK', exact=True)
                reply.wait_for(timeout=45000)
                page.reload()
                reply.wait_for(timeout=30000)
                page.get_by_role('button', name='Preview ' + attachment, exact=True).wait_for(timeout=15000)
                page.screenshot(path=report / 'electron-native-chat.png')
                workspace = page.locator('#talos-right-workspace')
                page.get_by_role('button', name=document.name, exact=True).click()
                expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text('Word preview first page', timeout=20000)
                page.screenshot(path=report / 'electron-remote-pdf.png')
                workspace.get_by_role('button', name='Close ' + document.name, exact=True).click()
                page.get_by_role('link', name='Worker report').click()
                expect(workspace.get_by_text('Report from the Talo Worker', exact=True)).to_be_visible(timeout=15000)
                workspace.get_by_role('button', name='Close talo-preview.txt', exact=True).click()
                page.evaluate("id => { location.hash = '/sessions/' + encodeURIComponent(id) }", session_id)
                page.get_by_role('tab', name='Conversation').wait_for()
                page.reload()
                reply.wait_for(timeout=30000)
                page.get_by_role('button', name='Preview ' + attachment, exact=True).wait_for(timeout=15000)
                page.get_by_role('button', name='Add', exact=True).click()
                page.get_by_role('button', name='Choose an existing file').click()
                page.get_by_role('button', name=attachment, exact=True).wait_for(timeout=15000)
                page.keyboard.press('Escape')
                personal_selection = page.evaluate(selection)
                assert personal_selection and talo_node not in personal_selection
                page.get_by_role('link', name='Files', exact=True).click()
                expect(page.get_by_text('Upload an image or document to reuse it in a conversation.', exact=True)).to_be_visible()
                expect(page.locator('.files-page-content').get_by_text(document.name, exact=True)).to_have_count(0)
                assert page.evaluate(selection) == personal_selection
                page.get_by_role('link', name='Talo', exact=True).click()
                page.get_by_role('button', name='Open Talo controls', exact=True).click()
                console = page.get_by_role('complementary', name='Manage Talo', exact=True)
                console.get_by_role('tab', name='Files', exact=True).click()
                console.get_by_role('button', name=document.name, exact=True).click()
                expect(workspace.locator('.react-pdf__Page__textContent:visible')).to_contain_text('Word preview first page', timeout=20000)
                page.screenshot(path=report / 'electron-talo-console-files.png')
                verify_web_history(playwright, base, session_id, prompt, report)
                page.get_by_role('link', name='New chat', exact=True).click()
                expect(page.get_by_role('heading', name='Start with one clear prompt', exact=True)).to_be_visible()
                assert page.evaluate(selection) == personal_selection
                print('PASS personal Files and Home keep their Computer selection and exclude Talo files', flush=True)

                # Additional Talo chats require an explicit target, independent of personal Home.
                data(http(base + f'/v1/fs/directory?nodeId={talo_node}&wakeIfSleeping=true'))
                additional_session = str(uuid.uuid4())
                data(http(base + '/v1/turns/start', {
                    'sessionId': additional_session, 'nodeId': talo_node,
                    'input': {'content': [{'type': 'text', 'text': 'native chat verification with an explicit Talo target'}]},
                    'modelSelection': sent.value.post_data_json.get('modelSelection'),
                    'clientRequestId': str(uuid.uuid4()),
                }))
                page.evaluate("id => { location.hash = '/sessions/' + encodeURIComponent(id) }", additional_session)
                page.get_by_text('TALO_CHAT_OK', exact=True).wait_for(timeout=30000)
                saved = data(http(base + '/v1/sessions/' + additional_session))['session']
                assert saved['sessionKind'] == 'talo' and saved['nodeId'] == talo_node

                def completed_additional_turn():
                    turns = data(http(base + '/v1/sessions/' + additional_session + '/history?limit=10'))['data']
                    return len(turns) == 1 and turns[0]['result']['status'] == 'completed'

                wait(completed_additional_turn, 'Explicit Talo first Turn persisted once')
                page.reload()
                page.get_by_text('TALO_CHAT_OK', exact=True).wait_for(timeout=30000)
                assert page.evaluate(selection) == personal_selection
                page.screenshot(path=report / 'electron-explicit-talo-chat.png')
                print('PASS explicit Talo target admits one Server-owned Turn; direct route and reload preserve personal selection', flush=True)
                print('PASS Talo Console lists the Talo file store and opens its Server-only PDF', flush=True)
                print('PASS native Talo attachment upload, streamed reply, remote PDF and Worker path preview, and reloaded image from both chat routes', flush=True)
                return
            page.get_by_role('button', name='Initialize Talo', exact=True).click()
            page.get_by_role('button', name='Connect desktop', exact=True).click(timeout=120000)
            wait_for_pixels(page)
            page.screenshot(path=report / 'electron-native-talo.png')
            page.get_by_role('button', name='Take control', exact=True).click()
            release = page.get_by_role('button', name='Return to agent', exact=True)
            release.wait_for()
            wait_for_pixels(page)
            from media import shift_pressed
            instance = data(http(base + ASSISTANT))['instance']['instanceId']
            worker, = managed_resource('container', instance)
            page.locator('canvas:visible').first.click()
            page.keyboard.down('Shift')
            try:
                wait(lambda: shift_pressed(worker), 'Electron keyboard input applied on the Talo Worker')
            finally:
                page.keyboard.up('Shift')
            release.click()
            page.get_by_role('button', name='Take control', exact=True).wait_for()
            print('PASS native Electron initialization, framebuffer, Worker keyboard input and control release', flush=True)
        except Exception:
            page.screenshot(path=report / 'electron-native-failure.png')
            print(page.locator('body').inner_text()[:1200], flush=True)
            raise
        return
    page.wait_for_url(lambda url: url.startswith(base + '/'), timeout=15000)
    streams, nodes = [], []

    def observe_request(request):
        if urlsplit(request.url).path == '/v1/desktop/open':
            nodes.append(request.post_data_json['nodeId'])

    page.on('request', observe_request)
    page.on('websocket', lambda ws: streams.append(urlsplit(ws.url)))
    assert page.evaluate('typeof window.talos') == 'undefined'
    electron_version = page.evaluate(r"navigator.userAgent.match(/Electron\/[\d.]+/)?.[0]")
    assert electron_version
    try:
        sign_in(page)
        page.goto(base + '/talo')
        connect = page.get_by_role('button', name='Connect desktop', exact=True)
        connect.wait_for(timeout=22000)
        if stage == 'before-reset':
            page.screenshot(path=report / 'electron-talo-chooser.png')
        connect.click()
        wait_for_pixels(page)
        media = [stream for stream in streams if stream.path == '/v1/desktop/stream']
        assert node_id in nodes and media and all(stream.netloc == urlsplit(base).netloc for stream in media)
        page.screenshot(path=report / f'electron-talo-{stage}.png')
        (report / f'electron-talo-{stage}.json').write_text(json.dumps({
            'stage': stage, 'electron': electron_version, 'nodeId': node_id, 'sameOriginDesktopRelay': True,
            'framebufferPixels': True, 'endpoints': [stream.path for stream in media],
        }, indent=2))
    except Exception:
        page.screenshot(path=report / f'electron-{stage}-failure.png', timeout=3000)
        raise


def main():
    if sys.argv[1:] == ['--check']:
        electron()
        return
    base, node_id, report, stage = sys.argv[1:]
    report = Path(report)
    debug_port = port()
    with tempfile.TemporaryDirectory(prefix='talos-electron-') as home, (report / f'electron-{stage}.log').open('wb') as log:
        env = dict(os.environ, HOME=home, LANG='en_US.UTF-8')
        if stage in ('initialize', 'chat', 'goal', 'loop'):
            env.pop('TALOS_DESKTOP_SERVER_URL', None)
            env.pop('TALOS_DESKTOP_RENDERER_URL', None)
        else:
            env['TALOS_DESKTOP_SERVER_URL'] = base
        process = subprocess.Popen([
            str(electron()), str(DESKTOP), '--lang=en-US', f'--remote-debugging-port={debug_port}', f'--user-data-dir={home}',
        ], env=env,
            stdout=log, stderr=log, start_new_session=True)
        owned_group = {
            'value': str(process.pid),
            'started': subprocess.run(['ps', '-p', str(process.pid), '-o', 'lstart='],
                                      capture_output=True, text=True, check=False).stdout.strip(),
        }
        try:
            with sync_playwright() as playwright:
                capture(playwright, process, f'http://127.0.0.1:{debug_port}', base, node_id, report, stage)
        finally:
            # Electron exiting does not prove its app-server child has stopped.
            # Drain the owned group before either temporary Home is removed.
            stop_group(owned_group)
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
