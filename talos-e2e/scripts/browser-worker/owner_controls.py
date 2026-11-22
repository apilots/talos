"""Owner controls through packaged Web and a cold, signed-in Desktop App Server."""

import base64
from contextlib import ExitStack
import json
import os
from pathlib import Path
import selectors
import sqlite3
import subprocess
import tempfile
import time
from urllib.parse import urlencode
import uuid

from playwright.sync_api import expect, sync_playwright
import yaml

from fixture import REPORT, ROOT, TOKEN, config, data, free_port, http, http_response, wait, start_ingress, stop
from lifecycle import container_args
from media import Viewer
from owner_controls_issuer import QA_TOKEN

JOBS = {'profileUsageRetention', 'dreamMemoryMaintenance'}
RETENTION = 'profileUsageRetention'


def start_server(stack, directory):
    web_port, rpc_port, issuer_port, ingress_port = (free_port() for _ in range(4))
    web = f'http://127.0.0.1:{web_port}'
    issuer = f'http://127.0.0.1:{issuer_port}'
    ingress = f'https://127.0.0.1:{ingress_port}'
    cert, key = directory / 'tls.crt', directory / 'tls.key'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
        '-days', '1', '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=IP:127.0.0.1',
        '-addext', 'basicConstraints=critical,CA:FALSE',
        '-addext', 'keyUsage=critical,digitalSignature,keyEncipherment',
        '-addext', 'extendedKeyUsage=serverAuth', '-keyout', str(key), '-out', str(cert)],
        check=True, capture_output=True)
    key.chmod(0o644)  # Disposable certificate read by the packaged non-root user.
    profile = config(directory, 8080, 18080)
    profile['gateway']['host'] = '0.0.0.0'
    profile['cloud'].update(agent_base_url='https://127.0.0.1:9443',
                            agent_trusted_ca_file='/fixture/tls.crt', web_base_url=issuer)
    profile['agent_node']['control_plane'] = {
        'bind_address': '0.0.0.0:9443', 'tls_certificate_file': '/fixture/tls.crt',
        'tls_private_key_file': '/fixture/tls.key', 'trusted_node_ca_file': '/fixture/tls.crt',
    }
    profile['sync']['hub_endpoint'] = 'wss://127.0.0.1:9/v1/sync'
    (directory / 'talos.yaml').write_text(yaml.safe_dump(profile))
    name = 'talos-owner-controls-' + uuid.uuid4().hex[:10]

    def cleanup():
        with (REPORT / 'server.log').open('wb') as log:
            subprocess.run(['docker', 'logs', name], stdout=log, stderr=subprocess.STDOUT, timeout=15)
        subprocess.run(['docker', 'rm', '-fv', name], capture_output=True, timeout=30)

    stack.callback(cleanup)
    command = ['docker', 'run', '-d', *container_args(name), '--shm-size=1g',
        '--security-opt', 'no-new-privileges:true', '--security-opt',
        'seccomp=' + str(ROOT / 'deploy/docker/talos-worker/chromium-seccomp.json'),
        '-p', f'127.0.0.1:{web_port}:8080', '-p', f'127.0.0.1:{rpc_port}:9443',
        '-p', f'127.0.0.1:{issuer_port}:18080',
        '-v', f'{directory}:/fixture:ro', '-v', f'{ROOT / "talos-e2e"}:/tests:ro',
        '-e', 'PYTHONPATH=/tests/scripts', '-e', 'TALOS_E2E_TMP_ROOT=/tmp/owner-controls',
        '-e', 'TALOS_SCENARIO_REPORT_DIR=/tmp/owner-controls',
        '-e', 'TALOS_HOME=/home/talos/.talos',
        '-e', 'TALOS_CONFIG=/home/talos/.talos/conf/talos.yaml',
        '-e', f'OWNER_CONTROLS_ISSUER={issuer}', os.environ['TALOS_E2E_SERVER_IMAGE'],
        'python3', '/tests/scripts/browser-worker/owner_controls_issuer.py']
    subprocess.run(command, check=True, stdout=subprocess.DEVNULL, timeout=60)
    wait(lambda: http(web + '/health').get('status') == 'healthy', 'Authenticated packaged Server')
    start_ingress(stack, directory, ingress, web, rpc_port, cert, key)
    return web, issuer, ingress, cert


def open_request(computer, snapshot):
    return {'nodeId': computer, 'targetId': 'managed', 'viewerId': str(uuid.uuid4()),
            'generation': snapshot['control']['generation'], 'inputMode': 'readOnly',
            'controllerId': None, 'inputEpoch': None}


def checked_jobs(response, computer):
    rows = response['data']
    assert {row['id'] for row in rows} == JOBS, rows
    assert all(row['computerId'] == computer and row['executionInstanceId'] for row in rows), rows
    return next(row for row in rows if row['id'] == RETENTION)


def controls(computer, list_jobs, control):
    job = checked_jobs(list_jobs(), computer)
    binding = {'computerId': computer, 'executionInstanceId': job['executionInstanceId'],
               'jobId': job['id']}
    assert control(dict(binding, action='pause'))['paused']
    assert control(dict(binding, action='runOnce'))['paused']

    def finished():
        current = checked_jobs(list_jobs(), computer)
        return (current['paused'] and not current['running']
                and (current['lastOutcome'] or {}).get('type') == 'succeeded')

    wait(finished, 'Owner Run once succeeds while paused', seconds=12)
    assert not control(dict(binding, action='resume'))['paused']
    return binding


def sign_in(context, page, web, account):
    page.goto(web, wait_until='domcontentloaded')
    button = page.get_by_role('button', name='Continue with Talos Cloud')
    with context.expect_page() as popup:
        button.click()
    popup.value.get_by_role('button', name=account, exact=True).click()
    expect(button).not_to_be_visible(timeout=20_000)
    page.wait_for_function("!location.pathname.startsWith('/auth/')")


def web_acceptance(web, computer, display_name):
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=os.environ.get('TALOS_E2E_CHROME_BIN') or
            '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless=True)
        try:
            admin = browser.new_context(locale='en-US', viewport={'width': 1440, 'height': 940})
            page = admin.new_page()
            sign_in(admin, page, web, 'Administrator')
            page.get_by_role('link', name='Automation', exact=True).click()
            with page.expect_response(lambda response: response.url.endswith('/v1/builtin-job/list')) as found:
                page.get_by_role('button', name='Built-in jobs', exact=True).click()
            job = checked_jobs(data(found.value.json()), computer)
            row = page.get_by_role('article').filter(has=page.get_by_role('heading', name=job['name'], exact=True))
            expect(page.get_by_role('article')).to_have_count(2)
            for label, action, paused in [('Pause', 'pause', True), ('Run once', 'runOnce', True),
                                          ('Resume', 'resume', False)]:
                with page.expect_response(lambda response: response.url.endswith('/v1/builtin-job/control')) as result:
                    row.get_by_role('button', name=label, exact=True).click()
                sent = result.value.request.post_data_json
                assert sent == {'computerId': computer, 'executionInstanceId': job['executionInstanceId'],
                                'jobId': RETENTION, 'action': action}
                assert data(result.value.json())['paused'] is paused
                if action == 'runOnce':
                    expect(row.get_by_text('Succeeded', exact=True)).to_be_visible(timeout=15_000)
            page.screenshot(path=REPORT / 'web-builtin-jobs.png')
            page.goto(web + '/computers', wait_until='domcontentloaded')
            view = page.get_by_role('button', name='View computer', exact=True)
            expect(page.get_by_role('heading', name='Connection topology')).to_be_visible()
            page.get_by_role('button', name=f'Inspect {display_name}', exact=True).click()
            expect(view).to_be_visible(timeout=20_000)
            view.click()
            expect(page.get_by_text('View only', exact=True)).to_be_visible(timeout=20_000)
            page.wait_for_function('''() => [...document.querySelectorAll("canvas")].some(canvas => {
                if (canvas.width < 20 || canvas.height < 20) return false;
                const ctx = canvas.getContext("2d");
                if (!ctx) return false;
                const pixels = new Set();
                for (let i = 0; i < 64; i++) {
                    pixels.add(String(ctx.getImageData(Math.floor(canvas.width * (i % 8) / 8),
                        Math.floor(canvas.height * Math.floor(i / 8) / 8), 1, 1).data));
                }
                return pixels.size > 3;
            })''', timeout=20_000)
            page.screenshot(path=REPORT / 'web-embedded-desktop.png')
            admin.close()
            standard = browser.new_context(locale='en-US')
            page = standard.new_page()
            sign_in(standard, page, web, 'Standard user')
            expect(page.get_by_role('heading', name='Choose a computer')).to_be_visible()
            standard.close()
        finally:
            browser.close()


class Desktop:
    def __init__(self, stack, home, issuer, ingress, cert):
        profile = config(home, 8080, int(issuer.rsplit(':', 1)[1]))
        profile['cloud'].update(agent_base_url=ingress, agent_trusted_ca_file=str(cert))
        profile['sync']['hub_endpoint'] = 'wss://127.0.0.1:9/v1/sync'
        path = home / 'conf/talos.yaml'
        path.parent.mkdir()
        path.write_text(yaml.safe_dump(profile))
        self.home, self.issuer, self.token = home, issuer, None
        self.sequence, self.pending, self.frames = 0, b'', []
        log = stack.enter_context((REPORT / f'{home.name}.log').open('wb'))
        self.process = subprocess.Popen(['node', str(REPORT / 'desktop-host.mjs')],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
            env=dict(os.environ, HOME=str(home), TALOS_HOME=str(home), TALOS_CONFIG=str(path),
                     TALOS_DESKTOP_HOSTNAME=home.name, TALOS_E2E_DESKTOP_MEDIA='1',
                     TALOS_APP_SERVER_BIN=os.environ['TALOS_E2E_APP_SERVER_BIN']))
        self.selector = stack.enter_context(selectors.DefaultSelector())
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        stack.callback(self.close)
        self.call('connect')

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                stop(self.process)
        log = self.home / 'logs/talos.log'
        if log.exists():
            (REPORT / f'{self.home.name}-app-server.log').write_bytes(log.read_bytes())

    def receive(self, deadline):
        while b'\n' not in self.pending and time.monotonic() < deadline:
            if self.selector.select(max(0, deadline - time.monotonic())):
                chunk = os.read(self.process.stdout.fileno(), 65536)
                assert chunk, 'Desktop App Server bridge closed unexpectedly'
                self.pending += chunk
        assert b'\n' in self.pending, 'Desktop App Server bridge timed out'
        line, self.pending = self.pending.split(b'\n', 1)
        return json.loads(line)

    def exchange(self, method, params=None):
        self.sequence += 1
        request = {'id': self.sequence, 'method': method, 'params': params, 'actorToken': self.token}
        self.process.stdin.write(json.dumps(request).encode() + b'\n')
        self.process.stdin.flush()
        deadline = time.monotonic() + 30
        while True:
            event = self.receive(deadline)
            if 'frame' in event:
                self.frames.append(event['frame'])
                continue
            assert event['id'] == self.sequence, f'Desktop response correlation failed for {method}'
            return event

    def call(self, method, params=None):
        event = self.exchange(method, params)
        assert 'result' in event, (method, event.get('error', {}).get('code'))
        return event['result']

    def login(self, account, role):
        started = self.call('auth/login/start', {'client_name': self.home.name, 'client_kind': 'desktop'})
        query = urlencode({'session_id': started['login_id'], 'account': account})
        assert http(self.issuer + '/device?' + query)['status'] == 'approved'
        result = self.call('auth/login/status', {'login_id': started['login_id']})
        assert result['status'] == 'authenticated' and result['user']['role'] == role
        self.token = result['access_token']

    def has_computer(self, computer):
        with sqlite3.connect(f'file:{self.home / "data/talos.db"}?mode=ro', uri=True) as db:
            return db.execute('SELECT 1 FROM agent_nodes WHERE node_id=?', (computer,)).fetchone() is not None


class DesktopViewer(Viewer):
    def __init__(self, desktop, grant):
        self.desktop, self.grant = desktop, grant
        self.pending, self.socket = bytearray(), self
        desktop.call('desktop/streamOpen', {key: grant[key] for key in ('streamId', 'ticket')})
        self.initialize()

    def recv(self, timeout):
        frame = (self.desktop.frames.pop(0) if self.desktop.frames else
                 self.desktop.receive(time.monotonic() + timeout)['frame'])
        assert frame['streamId'] == self.grant['streamId'] and frame['dataBase64']
        return base64.b64decode(frame['dataBase64'], validate=True)

    def send(self, payload):
        self.desktop.call('desktop/streamSend', {'streamId': self.grant['streamId'],
                          'dataBase64': base64.b64encode(payload).decode()})

    def close(self):
        self.desktop.call('desktop/streamClose', {'streamId': self.grant['streamId']})


def denied(response, codes):
    assert response['error']['code'] in codes, response


def main():
    with ExitStack() as stack:
        directory = Path(stack.enter_context(tempfile.TemporaryDirectory(
            prefix='owner-controls-', dir=os.environ.get('TALOS_E2E_TMP_ROOT'))))
        # Mount only the fixture files, not Desktop account data.
        server_files = directory / 'server'
        server_files.mkdir()
        web, issuer, ingress, cert = start_server(stack, server_files)
        assert http_response(web + '/v1/computers', token='invalid')[0] == 401
        assert data(http(web + '/v1/user'))['user']['role'] == 'admin'
        assert data(http(web + '/v1/user', token=QA_TOKEN))['user']['role'] == 'user'

        def owned_computer():
            listing = data(http(web + '/v1/computers'))['data']
            return next((row for row in listing if row['nodeId'] != 'embedded' and row['available']), None)

        owner_node = wait(owned_computer, 'Registered embedded Worker')
        computer = owner_node['nodeId']
        assert owner_node['ownerUserId'] == data(http(web + '/v1/user'))['user']['user_id']
        assert not data(http(web + '/v1/computers', token=QA_TOKEN))['data']
        web_acceptance(web, computer, owner_node['displayName'])
        print('CASE authenticated_web_owner_jobs_and_pixels PASS', flush=True)

        admin = Desktop(stack, directory / 'desktop-admin', issuer, ingress, cert)
        admin.login('Administrator', 'admin')
        assert admin.call('sync/status')['connection'] == 'disabled'
        def cached_computer():
            rows = admin.call('computer/list', {})['data']
            return next((row for row in rows if row['nodeId'] == computer), None)

        cached = wait(cached_computer, 'Computer discovered from Server with Sync disabled')
        assert cached['displayName'] == owner_node['displayName']
        confirmed_name = 'Renamed from Desktop with Sync disabled'
        renamed = admin.call('computer/rename', {'nodeId': computer, 'displayName': confirmed_name})
        assert renamed['displayName'] == confirmed_name
        assert cached_computer()['displayName'] == confirmed_name
        assert admin.call('computer/read', {'nodeId': computer})['displayName'] == confirmed_name
        server_name = 'Renamed directly at Server'
        code, response = http_response(web + f'/v1/computers/{computer}/name',
            {'nodeId': computer, 'displayName': server_name}, method='PATCH')
        assert code == 200 and data(response)['displayName'] == server_name
        wait(lambda: cached_computer()['displayName'] == server_name,
             'Periodic directory refresh after a direct Server rename')
        assert admin.call('sync/status')['connection'] == 'disabled'
        print('CASE no_sync_directory_and_bidirectional_rename_refresh PASS', flush=True)
        binding = controls(computer,
            lambda: admin.call('builtinJob/list', {'computerId': computer}),
            lambda request: admin.call('builtinJob/control', request))
        assert admin.has_computer(computer), 'Server directory cache lost the registered Computer'
        read = {'nodeId': computer, 'operation': {'kind': 'read'}}
        snapshot = admin.call('browser/operate', read)
        assert snapshot['display']['kind'] == 'dockerDesktop'
        assert admin.has_computer(computer), 'Computer connection changed the Server directory cache'
        viewer = DesktopViewer(admin, admin.call('desktop/open', open_request(computer, snapshot)))
        try:
            viewer.frame(REPORT / 'desktop-embedded-desktop.png')
        finally:
            viewer.close()
        print('CASE cold_desktop_owner_jobs_and_pixels PASS', flush=True)

        qa = Desktop(stack, directory / 'desktop-standard', issuer, ingress, cert)
        qa.login('Standard user', 'user')
        blocked = [('builtinJob/list', '/v1/builtin-job/list', {'computerId': computer}),
                   ('builtinJob/control', '/v1/builtin-job/control', dict(binding, action='pause')),
                   ('desktop/open', '/v1/desktop/open', open_request(computer, snapshot))]
        for method, path, request in blocked:
            codes = {'permissionDenied', 'resourceNotFound'} if method == 'desktop/open' else {'permissionDenied'}
            denied(qa.exchange(method, request), codes)
            status, response = http_response(web + path, request, QA_TOKEN)
            assert status in (403, 404)
            denied(response, codes)
        denied(qa.exchange('browser/operate', read), {'permissionDenied', 'resourceNotFound'})
        status, response = http_response(web + '/v1/builtin-job/control',
            dict(binding, action='pause', executionInstanceId='stale-fixture-instance'))
        assert status == 409
        denied(response, {'conflict'})
        assert not checked_jobs(admin.call('builtinJob/list', {'computerId': computer}), computer)['paused']

        def standard_computer():
            status = qa.call('sync/status')
            assert status['connection'] == 'disabled'
            node = status['localComputerId']
            owned = data(http(web + '/v1/computers', token=QA_TOKEN))['data']
            return node if node not in (None, 'embedded') and any(
                row['nodeId'] == node and row['available'] for row in owned) else None

        qa_computer = wait(standard_computer, 'Standard user registered their own Worker')
        qa_binding = controls(qa_computer,
            lambda: qa.call('builtinJob/list', {'computerId': qa_computer}),
            lambda request: qa.call('builtinJob/control', request))
        denied(admin.exchange('builtinJob/control', dict(qa_binding, action='pause')), {'permissionDenied'})
        print('CASE authenticated_owner_isolation_and_regular_worker_parity PASS', flush=True)


if __name__ == '__main__':
    main()
