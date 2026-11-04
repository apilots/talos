"""JSON-driven single-Turn load acceptance in the production Web UI.

Session content and SSE are explicit contract fixtures; metadata, Computer
selection and static assets use the real Server. No model or production data.
"""
import json
import os
from pathlib import Path
import re
import socket
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

import yaml
from playwright.sync_api import sync_playwright, expect
from model_protocol_fixture import model

ROOT = Path(__file__).resolve().parents[2]
REPORT = Path(os.environ['TALOS_SCENARIO_REPORT_DIR'])
OFFSET = 2_000_000
SEQUENCE = 1_900_000
LONG, SHORT = 'json-long-turn', 'json-short-session'


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def snapshot(steps):
    started = int(time.time()) - 3 * 3600
    def block(key, content, running=False):
        return dict(blockId=key, status='running' if running else 'completed', content=content,
                    startedAt=started, completedAt=None if running else started + 1)
    def message(key, role, blocks, running=False):
        return dict(messageId=key, turnId='long-turn', role=role, blocks=blocks,
                    status='inProgress' if running else 'completed', createdAt=started,
                    completedAt=None if running else started + 1)
    messages = [message('input', 'user', [block('input', {'type': 'userContent',
                 'data': {'type': 'text', 'text': 'A normal three-hour single Turn'}})])]
    for index in range(steps):
        messages.append(message(f'model-{index}', 'agent', [
            block(f'thinking-{index}', {'type': 'reasoning', 'data': {'text': f'Step {index}: ' + 'detail ' * 128}}),
            block(f'call-{index}', {'type': 'toolCall', 'data': {'callId': f'call-{index}',
                  'name': 'Bash', 'arguments': {'state': 'complete', 'value': {'command': f'echo step-{index}'}}, 'progress': []}}),
        ]))
        messages.append(message(f'result-{index}', 'tool', [block(f'result-{index}', {
            'type': 'toolResult', 'data': {'callId': f'call-{index}', 'toolName': 'Bash',
            'metrics': None, 'isError': False, 'content': [{'type': 'text', 'text': f'Result {index}: ' + 'output ' * 128}]},
        })]))
    messages.append(message('live', 'agent', [block('tail', {'type': 'reasoning', 'data': {'text': 'SNAPSHOT_READY'}}, True)], True))
    usage = dict(input_tokens=0, output_tokens=0, total_tokens=0, cache_creation_input_tokens=None, cache_read_input_tokens=None)
    return dict(identity={'sessionId': LONG, 'turnId': 'long-turn'}, initialMessageId='input',
                state='running', lastEventId=f'ev_{OFFSET:020d}', lastEventSeq=OFFSET,
                lastSequence=SEQUENCE, updatedAt=int(time.time()), messages=messages,
                metrics=dict(modelRounds=steps + 1, toolCalls=steps, toolSuccesses=steps,
                             toolErrors=0, usage=usage, context=None, durationMs=10800000),
                pendingInput=None, notices=[])


class Data:
    def __init__(self, steps):
        self.turn = snapshot(steps)
        self.events = []
        self.requests = []
        self.stale = []
        self.stopped = threading.Event()
        self.refresh()

    def refresh(self):
        self.raw = json.dumps({'status': 'success', 'data': {
            'activeTurn': self.turn, 'lastCompletedEventSeq': 0, 'latestEventId': self.turn['lastEventId']}}).encode()

    def burst(self, count=1000):
        for index in range(count + 1):
            text = 'x' if index < count else 'LIVE_TAIL'
            seq = OFFSET + index + 1
            self.events.append(dict(eventId=f'ev_{seq:020d}', eventSeq=seq, persistedAt=int(time.time()),
                event={'type': 'agent', 'data': {'identity': self.turn['identity'],
                'sequence': SEQUENCE + index + 1, 'occurredAt': int(time.time()),
                'payload': {'type': 'messageBlockDelta', 'data': {'messageId': 'live',
                'blockId': 'tail', 'delta': {'type': 'text', 'data': text}}}}}))
        self.turn['messages'][-1]['blocks'][0]['content']['data']['text'] += 'x' * count + 'LIVE_TAIL'
        self.turn.update(lastEventId=self.events[-1]['eventId'], lastEventSeq=self.events[-1]['eventSeq'], lastSequence=SEQUENCE + count + 1)
        self.refresh()


def event_handler(fixture):
    class Events(BaseHTTPRequestHandler):
        def log_message(self, *_args): pass
        def do_GET(self):
            query = parse_qs(urlparse(self.path).query)
            cursor = query.get('after_event_id', [''])[0]
            idle = urlparse(self.path).path == '/idle'
            if not idle: fixture.requests.append(cursor)
            after = int(cursor.removeprefix('ev_') or 0)
            if not idle and after < OFFSET: fixture.stale.append(cursor)
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Access-Control-Allow-Origin', '*')
            self.end_headers()
            try:
                while not fixture.stopped.wait(0.05):
                    for event in [] if idle else fixture.events:
                        if event['eventSeq'] <= after: continue
                        raw = json.dumps(event)
                        self.wfile.write(f'event: timeline/message/block/delta\nid: {event["eventId"]}\ndata: {raw}\n\n'.encode())
                        after = event['eventSeq']
                    self.wfile.write(b': keepalive\n\n')
                    self.wfile.flush()
            except (BrokenPipeError, ConnectionResetError): pass
    return Events


def session(identity, fixture):
    return dict(id=identity, title='Long JSON session' if identity == LONG else 'Short JSON session',
        nodeId='embedded', userId='anonymous', sourceType='http', sourceProvider='http',
        modelSelection={'modelRef': {'provider': 'json-fixture', 'id': 'opaque-7f82'}, 'reasoningEffort': None},
        configuredDefaultMode='fullAccess', effectiveMode='fullAccess', createdAt=int(time.time() * 1000) - 10800000,
        lastActivity=int(time.time() * 1000), messageCount=len(fixture.turn['messages']) if identity == LONG else 0,
        usage={'tokens': fixture.turn['metrics']['usage'], 'context': None}, projectId=None, currentDirectory=None)


def route_content(route, fixture, event_origin):
    parsed = urlparse(route.request.url)
    path = parsed.path
    if path == '/v1/sessions':
        body = {'sessions': [{**session(identity, fixture), 'hasActiveTurn': identity == LONG} for identity in (LONG, SHORT)]}
    elif path in (f'/v1/sessions/{LONG}', f'/v1/sessions/{SHORT}'):
        body = {'session': session(path.rsplit('/', 1)[1], fixture)}
    elif path.endswith('/active-turn'):
        if LONG in path:
            route.fulfill(body=fixture.raw, content_type='application/json'); return
        body = {'activeTurn': None, 'lastCompletedEventSeq': 0, 'latestEventId': None}
    elif path.endswith('/history'):
        body = {'data': [], 'nextCursor': None, 'lastStableEventId': None}
    elif path.endswith('/events'):
        if LONG in path:
            route.continue_(url=event_origin + '/events?' + parsed.query); return
        route.continue_(url=event_origin + '/idle'); return
    elif path.startswith('/v1/message-queue/'):
        body = {'revision': 1, 'maxMessages': 100, 'messages': []}
    else:
        route.continue_(); return
    route.fulfill(json={'status': 'success', 'data': body})


def profile(browser, origin, steps):
    fixture = Data(steps)
    snapshot_bytes = len(fixture.raw)
    REPORT.joinpath(f'turn-{steps}.json').write_bytes(fixture.raw)
    events = ThreadingHTTPServer(('127.0.0.1', 0), event_handler(fixture))
    events.daemon_threads = True
    threading.Thread(target=events.serve_forever, daemon=True).start()
    context = browser.new_context(viewport={'width': 1440, 'height': 1000})
    context.add_init_script('localStorage.setItem("talos-web-ui",JSON.stringify({state:{locale:"en",uiTheme:"light"},version:0}));window.loadTasks=[];new PerformanceObserver(l=>window.loadTasks.push(...l.getEntries().map(e=>({start:e.startTime,duration:e.duration})))).observe({type:"longtask",buffered:true})')
    page = context.new_page()
    page.route('**/v1/**', lambda route: route_content(route, fixture, f'http://127.0.0.1:{events.server_port}'))
    page.route('https://fonts.googleapis.com/**', lambda route: route.abort())
    errors = []
    page.on('pageerror', lambda error: errors.append(str(error)))
    try:
        began = time.monotonic()
        page.goto(origin + '/sessions/' + LONG)
        tail = page.locator('[data-timeline-item-key$=":tail"] [data-timeline-text]')
        expect(tail).to_have_text('SNAPSHOT_READY', timeout=30000)
        cold_ms = (time.monotonic() - began) * 1000
        page.wait_for_timeout(100)
        assert fixture.requests and not fixture.stale, fixture.requests
        initial_requests = list(fixture.requests)
        cold_tasks = page.evaluate("window.loadTasks")
        mounted = page.locator('[data-process-step]').count()
        assert mounted <= 64, mounted
        if steps >= 64:
            page.get_by_role('button', name=re.compile('Show .* earlier steps')).click()
            assert page.locator('[data-process-step]').count() > mounted
        page.reload()
        expect(tail).to_have_text('SNAPSHOT_READY')
        fixture.burst()
        expect(tail).not_to_have_text('SNAPSHOT_READY', timeout=30000)
        began = time.monotonic()
        page.get_by_role('button', name=re.compile('^Short JSON session')).click()
        page.wait_for_url('**/sessions/' + SHORT)
        switch_ms = (time.monotonic() - began) * 1000
        assert switch_ms < 1500, switch_ms
        page.get_by_role('button', name=re.compile('^Long JSON session')).click()
        expected = 'SNAPSHOT_READY' + 'x' * 1000 + 'LIVE_TAIL'
        expect(tail).to_have_text(expected, timeout=30000)
        assert not errors, errors
        assert not fixture.stale, fixture.stale
        tasks = page.evaluate('window.loadTasks')
        page.screenshot(path=str(REPORT / f'turn-{steps}.png'))
        return dict(steps=steps, messages=len(fixture.turn['messages']),
            blocks=sum(len(message['blocks']) for message in fixture.turn['messages']),
            jsonBytes=snapshot_bytes, representedHours=3, snapshotOffset=OFFSET,
            coldReadyMs=round(cold_ms, 1), switchDuringBurstMs=round(switch_ms, 1),
            initiallyMountedSteps=mounted, resumedOffsets=initial_requests,
            finalTextExact=True, staleReplayRequests=fixture.stale, pageErrors=errors,
            coldLongTasks=cold_tasks, reloadAndBurstLongTasks=tasks)
    finally:
        fixture.stopped.set(); context.close(); events.shutdown(); events.server_close()


def main():
    REPORT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='talos-turn-json-') as home:
        port = free_port()
        config = yaml.safe_load((ROOT / 'conf/talos-dev.yaml').read_text())
        config['gateway'].update(host='127.0.0.1', port=port, auth=False)
        config['llm'].update(provider='json-fixture', model='opaque-7f82', custom_providers={
            'json-fixture': dict(name='JSON fixture', protocol='chat', api_key='local-unused',
                base_url='http://127.0.0.1:1/v1', models=[model('chat', 'protocol_default')])})
        config['agent']['inner_model'] = {'provider': 'json-fixture', 'id': 'opaque-7f82'}
        config_path = Path(home) / 'talos.yaml'
        config_path.write_text(yaml.safe_dump(config))
        env = {**os.environ, 'TALOS_HOME': home, 'TALOS_CONFIG': str(config_path),
               'TALOS_RESOURCE_DIR': str(ROOT), 'TALOS_WEBUI_DIR': str(ROOT / 'talos-app/web/dist')}
        with REPORT.joinpath('server.log').open('w') as log:
            server = subprocess.Popen([os.environ['TALOS_E2E_SERVER_BIN']], env=env, stdout=log, stderr=log)
            try:
                origin = f'http://127.0.0.1:{port}'
                for _ in range(200):
                    try:
                        with urllib.request.urlopen(origin + '/health', timeout=1): break
                    except OSError:
                        assert server.poll() is None, 'Server exited; inspect server.log'
                        time.sleep(0.1)
                else: raise AssertionError('Server startup timed out')
                with sync_playwright() as playwright:
                    browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN',
                        '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
                    try:
                        results = []
                        for steps in (8, 500, 3000):
                            result = profile(browser, origin, steps)
                            results.append(result)
                            REPORT.joinpath('performance.json').write_text(json.dumps(results, indent=2))
                            print(json.dumps({key: value for key, value in result.items() if key != 'longTasks'}), flush=True)
                    finally: browser.close()
                print('CASE running_turn_json PASS cold_offset=true content_exact=true navigation=responsive')
            finally:
                server.terminate()
                try: server.wait(timeout=15)
                except subprocess.TimeoutExpired: server.kill(); server.wait()


if __name__ == '__main__': main()
