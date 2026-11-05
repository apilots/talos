"""Real Web slash/Add and prompt activation against the shared secretless model fixture."""
import json
import os
from pathlib import Path
import re
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.request
import uuid
from urllib.parse import urlsplit

import yaml
from playwright.sync_api import expect, sync_playwright

from automation_model_fixture import prepare

ROOT = Path(__file__).resolve().parents[2]
REPORT = Path(os.environ['TALOS_SCENARIO_REPORT_DIR'])


def wait_for(probe, description, seconds=40):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = probe()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(f'{description} timed out')


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def definition(database, mode, marker):
    table, field = ('session_goals', 'objective') if mode == 'goal' else ('session_loops', 'prompt')
    with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as db:
        db.row_factory = sqlite3.Row
        row = db.execute(f'SELECT * FROM {table} WHERE {field} LIKE ?', (f'%{marker}%',)).fetchone()
        return dict(row) if row else None


def user_inputs(database, session):
    with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as db:
        return [json.loads(row[0])['data'] for row in db.execute(
            "SELECT payload_json FROM agent_events WHERE session_id=? AND json_extract(payload_json,'$.type')='messageCompleted'",
            (session,)) if json.loads(row[0])['data']['role'] == 'user']


def assert_selection(contents, mode, prompt):
    assert any(item == {'type': 'text', 'text': prompt} for item in contents), contents
    selected = [item for item in contents if item['type'] == 'skillSelection']
    assert len(selected) == 1 and selected[0]['name'] == mode, contents
    assert '/.system/' + mode + '/' in selected[0]['path'], selected


def assert_input(contents, mode, entry, prompt):
    if entry == 'prompt':
        assert contents == [{'type': 'text', 'text': prompt}], contents
    else:
        assert_selection(contents, mode, prompt)


def begin(page, mode, entry, prompt):
    url = urlsplit(page.url)
    page.goto(f'{url.scheme}://{url.netloc}/')
    composer = page.get_by_role('textbox', name='Ask anything. Press / for commands and skills.')
    expect(composer).to_be_visible()
    if entry == 'add':
        page.get_by_role('button', name='Add', exact=True).click()
        page.get_by_role('button', name=mode.title(), exact=True).click()
    elif entry == 'slash':
        composer.fill('/' + mode)
        page.get_by_test_id('composer-suggestions').get_by_role('option', name=re.compile('^' + mode.title() + r'\b')).click()
    if entry != 'prompt':
        expect(page.get_by_test_id('automation-mode-chip').get_by_role('button')).to_have_text(mode.title())
    composer.fill(prompt)
    with page.expect_request(lambda request: request.method == 'POST' and request.url.endswith('/v1/turns/start')) as sent:
        page.get_by_role('button', name='Send', exact=True).click()
    request = sent.value.post_data_json
    assert_input(request['input']['content'], mode, entry, prompt)
    expect(page.get_by_test_id('automation-mode-chip')).to_have_count(0)
    return request


def queue_and_steer(page, session, turn):
    human = 'WEB_HUMAN_STEER_' + uuid.uuid4().hex
    composer = page.get_by_role('textbox', name='Ask anything. Press / for commands and skills.')
    composer.fill(human)
    with page.expect_response(lambda response: response.url.endswith('/v1/message-queue/add')) as queued:
        composer.press('Enter')
    messages = queued.value.json()['data']['messages']
    assert len(messages) == 1 and messages[0]['source']['type'] == 'user', messages
    url = urlsplit(page.url)
    with urllib.request.urlopen(f'{url.scheme}://{url.netloc}/v1/sessions/{session}/active-turn') as response:
        assert json.load(response)['data']['activeTurn']['identity']['turnId'] == turn
    waiting = page.get_by_test_id('waiting-messages')
    expect(waiting).to_contain_text(human)
    with page.expect_request(lambda request: request.url.endswith('/v1/turns/input')) as steered:
        waiting.get_by_role('button', name='Steer', exact=True).click()
    assert steered.value.post_data_json['identity']['turnId'] == turn
    assert steered.value.post_data_json['input'] == {'type': 'queuedMessage', 'messageId': messages[0]['id']}
    expect(waiting.get_by_text(human, exact=True)).to_have_count(0)
    user = page.get_by_test_id('user-input-message').filter(has_text=human)
    expect(user).to_be_visible()
    expect(user.get_by_test_id('automation-input-source')).to_have_count(0)


def complete(page, database, fixture, mode, entry):
    kind = 'goal' if mode == 'goal' else 'dynamicstop'
    marker = kind + '_' + uuid.uuid4().hex
    prompt = f'AUTO_CASE={marker} controlled automation work'
    if mode == 'loop':
        prompt += '; check progress; model decides the next check time'
    if entry == 'prompt':
        prompt = 'Please use Goal to finish this task: ' + prompt
    begin(page, mode, entry, prompt)
    screenshot = f'{mode}-{entry}'
    task = wait_for(lambda: definition(database, mode, marker), mode + ' registration')
    row = page.get_by_test_id('automation-task-' + task['id'])
    expect(row).to_be_visible()
    expect(row.locator('time')).to_have_text(re.compile(r'Starts in [1-9][0-9]?s'))
    page.screenshot(path=str(REPORT / f'{screenshot}-countdown.png'), full_page=True)
    wait_for(lambda: (fixture / f'{marker}.work-1').exists(), mode + ' automatic work')
    running = definition(database, mode, marker)
    assert running['latest_turn_id']
    with sqlite3.connect(f'file:{database}?mode=ro', uri=True) as db:
        started = db.execute("SELECT occurred_at FROM agent_events WHERE turn_id=? AND json_extract(payload_json,'$.type')='turnStarted'",
                             (running['latest_turn_id'],)).fetchone()[0]
    assert started >= task['created_at'] + 10, (task, started)
    expect(row).to_be_visible()
    expect(page.get_by_test_id('automation-input-source').last).to_have_text('Sent as ' + mode)
    page.screenshot(path=str(REPORT / f'{screenshot}-running.png'), full_page=True)
    if mode == 'goal' and entry != 'prompt':
        queue_and_steer(page, task['session_id'], running['latest_turn_id'])
    (fixture / f'{marker}.release-work-1').touch()
    if mode == 'loop':
        wait_for(lambda: (fixture / f'{marker}.declared').exists(), 'model Stop declaration')
        (fixture / f'{marker}.release-declared').touch()
    terminal = 'complete' if mode == 'goal' else 'stopped'
    finished = wait_for(lambda: (value := definition(database, mode, marker)) and value['status'] == terminal and value,
                        mode + ' terminal settlement')
    expect(row).to_have_count(0)
    answer = 'Goal completion declared' if mode == 'goal' else 'Model Stop declaration settles'
    expect(page.get_by_role('main')).to_contain_text(answer)
    inputs = user_inputs(database, finished['session_id'])
    original = next(message for message in inputs if any(block['content'].get('data', {}).get('text') == prompt
                    for block in message['blocks']))
    assert_input([block['content']['data'] for block in original['blocks'] if block['content']['type'] == 'userContent'], mode, entry, prompt)
    page.screenshot(path=str(REPORT / f'{screenshot}-ended.png'), full_page=True)
    return {'mode': mode, 'entry': entry, 'id': task['id'], 'status': terminal,
            'sessionId': task['session_id'], 'firstWorkDelaySeconds': started - task['created_at']}


def cancel(page, database, fixture):
    marker = 'dynamicstop_' + uuid.uuid4().hex
    prompt = f'AUTO_CASE={marker} controlled automation work; check progress; model decides the next check time'
    begin(page, 'loop', 'add', prompt)
    task = wait_for(lambda: definition(database, 'loop', marker), 'cancellable registration')
    row = page.get_by_test_id('automation-task-' + task['id'])
    expect(row.locator('time')).to_have_text(re.compile(r'Starts in [1-9][0-9]?s'))
    row.get_by_role('button', name='Cancel Loop', exact=True).click()
    expect(row).to_have_count(0)
    wait_for(lambda: definition(database, 'loop', marker)['cleared_at'], 'countdown clear')
    wait_for(lambda: time.time() >= task['created_at'] + 11, 'original first-work deadline')
    final = definition(database, 'loop', marker)
    assert final['latest_turn_id'] is None
    assert not (fixture / f'{marker}.work-1').exists()
    page.screenshot(path=str(REPORT / 'loop-countdown-cancelled.png'), full_page=True)
    return {'mode': 'loop', 'entry': 'add', 'id': task['id'], 'clearedBeforeFirstWork': True}


def run(home):
    fixture = home / 'fixture'
    fixture.mkdir()
    home.joinpath('.env').write_text('')
    processes = []
    logs = []
    try:
        def start(name, arguments, env=None):
            log = (REPORT / (name + '.log')).open('w')
            logs.append(log)
            process = subprocess.Popen(arguments, env=env, stdout=log, stderr=log)
            processes.append(process)
            return process
        model = start('model', ['python3', str(ROOT / 'talos-e2e/scripts/automation_model_fixture.py'), 'serve', str(fixture)])
        wait_for(lambda: (fixture / 'port').exists() if model.poll() is None else False, 'controlled upstream')
        config_path = home / 'talos.yaml'
        prepare(ROOT / 'conf/talos-dev.yaml', config_path, int((fixture / 'port').read_text()))
        config = yaml.safe_load(config_path.read_text())
        port = free_port()
        config['gateway'].update(host='127.0.0.1', port=port)
        config_path.write_text(yaml.safe_dump(config, sort_keys=False))
        env = {**os.environ, 'TALOS_HOME': str(home), 'TALOS_CONFIG': str(config_path),
               'TALOS_RESOURCE_DIR': str(ROOT), 'TALOS_WEBUI_DIR': str(ROOT / 'talos-app/web/dist')}
        server = start('server', [os.environ['TALOS_E2E_SERVER_BIN']], env)
        origin = f'http://127.0.0.1:{port}'
        def healthy():
            assert server.poll() is None, 'Real Server exited; inspect server.log'
            try:
                return urllib.request.urlopen(origin + '/health', timeout=1).status == 200
            except OSError:
                return False
        wait_for(healthy, 'real Server', 90)
        with sync_playwright() as playwright:
            browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN',
                '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'), headless=True)
            context = browser.new_context(viewport={'width': 1280, 'height': 900}, locale='en-US')
            context.add_init_script('localStorage.setItem("talos-web-ui",JSON.stringify({state:{locale:"en",uiTheme:"light"},version:0}));')
            page = context.new_page()
            page.goto(origin)
            try:
                results = [complete(page, home / 'data/talos.db', fixture, 'goal', 'add'),
                           complete(page, home / 'data/talos.db', fixture, 'loop', 'slash'),
                           cancel(page, home / 'data/talos.db', fixture),
                           complete(page, home / 'data/talos.db', fixture, 'goal', 'prompt')]
                assert not (fixture / 'errors.jsonl').exists(), 'Controlled upstream contract failed'
                (REPORT / 'result.json').write_text(json.dumps({'result': 'PASS', 'tasks': results}, indent=2))
                print('CASE automation_web_entry PASS server=real tool=real entries=Add,slash,prompt selection=typed-or-none countdown=real terminalRows=removed cancel=beforeWork')
            except Exception:
                page.screenshot(path=str(REPORT / 'failure.png'), full_page=True)
                raise
            finally:
                browser.close()
    finally:
        for process in reversed(processes):
            process.terminate()
            try:
                process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        for log in logs:
            log.close()
        for name in ('errors.jsonl', 'retired-streams.jsonl'):
            if (fixture / name).exists():
                shutil.copy2(fixture / name, REPORT / name)


if __name__ == '__main__':
    REPORT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='talos-automation-web-') as temporary:
        run(Path(temporary))
