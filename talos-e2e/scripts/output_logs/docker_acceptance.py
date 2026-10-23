"""Local acceptance: real Server, outbound Worker, and retained Docker volume.

Auth and model replies reuse the existing secretless protocol fixture. Tools,
transport, event persistence, process termination, and filesystem I/O are real.
"""
import hashlib
import io
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import threading
import time
import uuid
from http.server import ThreadingHTTPServer

import yaml

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
sys.path.insert(0, str(SCRIPTS / 'browser-worker'))
import fixture as base
from lifecycle import container_args, docker_create

ROOT, REPORT, RUN = base.ROOT, base.REPORT, base.RUN
IMAGE = os.environ['TALOS_E2E_WORKER_IMAGE']
PREFIX = '/home/talos/.talos/workspace-talos/' + base.USER['user_id']
MODEL_REPLIES = []


def tool_values(value):
    """Find domain projections in event envelopes without depending on nesting."""
    if isinstance(value, dict):
        yield value
        for child in value.values():
            yield from tool_values(child)
    elif isinstance(value, list):
        for child in value:
            yield from tool_values(child)
    elif isinstance(value, str) and value.startswith(('{', '[')):
        try:
            yield from tool_values(json.loads(value))
        except ValueError:
            pass


class Model(base.Fixture):
    def do_POST(self):
        if self.path != '/v1/chat/completions':
            return super().do_POST()
        raw = self.rfile.read(int(self.headers['Content-Length']))
        request = json.loads(raw)
        if not request.get('tools'):
            self.rfile = io.BytesIO(raw)
            return super().do_POST()
        messages = request['messages']
        last = max(i for i, message in enumerate(messages) if message['role'] == 'user')
        text = messages[last]['content']
        if isinstance(text, list):
            text = ''.join(block.get('text', '') for block in text)
        start = text.index('OUTPUT_LOG_TASK=') + len('OUTPUT_LOG_TASK=')
        task, _ = json.JSONDecoder().raw_decode(text[start:])
        replies = [message for message in messages[last + 1:] if message['role'] == 'tool']
        for reply in replies:
            size = len(reply['content'].encode())
            base.require(size <= 10000, f'Model tool projection bypassed cap: {size}')
        done = any(message.get('tool_call_id', '').startswith('output_log_') for message in replies)
        names = [tool['function']['name'] for tool in request['tools']]
        name = next((name for name in names if name.lower() == task['tool'].lower()), None)
        if done:
            MODEL_REPLIES.append({'task': task, 'replies': replies})
            delta, finish = {'role': 'assistant', 'content': 'OUTPUT_LOG_ACCEPTANCE_OK'}, 'stop'
        else:
            call_id = 'output_log_' + uuid.uuid4().hex
            args = task['args']
            if name is None:
                name = next(name for name in names if name.lower() == 'toolsearch')
                args, call_id = {'query': task['tool'], 'limit': 5}, 'discover_' + uuid.uuid4().hex
            delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': call_id, 'type': 'function',
                'function': {'name': name, 'arguments': json.dumps(args)}}]}
            finish = 'tool_calls'
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Connection', 'close')
        self.end_headers()
        for part in (delta, {}):
            value = {'id': 'output_log_fixture', 'object': 'chat.completion.chunk', 'created': int(time.time()),
                'model': base.MODEL['id'], 'choices': [{'index': 0, 'delta': part, 'finish_reason': finish if not part else None}]}
            self.wfile.write(('data: ' + json.dumps(value) + '\n\n').encode())
        self.wfile.write(b'data: [DONE]\n\n')
        self.wfile.flush()
        self.close_connection = True


class Acceptance:
    def __init__(self):
        self.name = 'talos-output-logs-' + uuid.uuid4().hex[:10]
        self.volume = self.name + '-home'
        self.url = 'http://127.0.0.1:' + str(base.free_port())
        self.rpc = base.free_port()
        self.mock = ThreadingHTTPServer(('0.0.0.0', 0), Model)
        self.server = None
        self.generation = 0
        self.evidence = []

    def proof(self, case, **details):
        self.evidence.append({'case': case, **details})
        (REPORT / 'docker-output-logs.json').write_text(json.dumps(self.evidence, indent=2))
        print('CASE ' + case + ' PASS', flush=True)

    def docker(self, *args):
        return subprocess.check_output(['docker', *args], text=True).strip()

    def volume_python(self, source, *args):
        return self.docker('run', '--rm', *container_args(self.name + '-inspect-' + uuid.uuid4().hex[:6]),
            '--user', '0', '-v', self.volume + ':/home/talos/.talos', '--entrypoint', 'python3', IMAGE, '-c', source, *args)

    def records(self):
        return json.loads(self.volume_python("""
from pathlib import Path
import json, sys
records = []
for path in Path(sys.argv[1]).glob('tool-output/*/*/*/metadata.json'):
    try:
        record = json.loads(path.read_text())
        try: size = (path.parent/'output.log').stat().st_size
        except FileNotFoundError: size = None
        records.append(dict(metadata=str(path), record=record, size=size))
    except FileNotFoundError:
        pass  # Background reclamation may complete after directory enumeration.
print(json.dumps(records))
""", PREFIX))

    def snapshot_tree(self, path):
        tree = self.docker('exec', self.name, 'python3', '-c', '''
from pathlib import Path
import os, stat, sys
root = Path(sys.argv[1])
def walk(directory, prefix=''):
    entries = sorted(directory.iterdir(), key=lambda p: (not p.is_dir(), p.name))
    for i, path in enumerate(entries):
        info = path.stat()
        last = i == len(entries)-1
        print(prefix + ('└── ' if last else '├── ') + path.name + ('/' if path.is_dir() else '')
              + f'  [uid={info.st_uid} mode={stat.S_IMODE(info.st_mode):04o}'
              + (f' bytes={info.st_size}' if path.is_file() else '') + ']')
        if path.is_dir(): walk(path, prefix + ('    ' if last else '│   '))
print(str(root) + '/')
walk(root)
''', PREFIX)
        inspect = json.loads(self.docker('inspect', self.name))[0]
        mount = next(mount for mount in inspect['Mounts'] if mount['Destination'] == '/home/talos/.talos')
        (REPORT / 'worker-workspace-tree.txt').write_text(
            'Container: ' + self.name + '\nVolume: ' + self.volume + '\nMount: ' + mount['Destination']
            + '\nDocker VM volume path: ' + mount['Source']
            + '\nComputer owner: ' + base.USER['user_id']
            + '\nCommand cwd: /tmp\nServer output.log copies: ' + str(len(list(self.home.rglob('output.log'))))
            + '\n\n' + tree + '\n\nFull output path:\n' + path + '\n')
        (REPORT / 'worker-volume-mount.json').write_text(json.dumps(mount, indent=2))

    def start(self):
        threading.Thread(target=self.mock.serve_forever, daemon=True).start()
        self.home = RUN / 'server'
        cert, key = RUN / 'tls.crt', RUN / 'tls.key'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=host.docker.internal',
            '-addext', 'subjectAltName=DNS:host.docker.internal,IP:127.0.0.1', '-addext', 'basicConstraints=critical,CA:FALSE',
            '-addext', 'keyUsage=critical,digitalSignature,keyEncipherment', '-addext', 'extendedKeyUsage=serverAuth',
            '-keyout', str(key), '-out', str(cert)], check=True, capture_output=True)
        config = base.config(self.home, int(self.url.rsplit(':', 1)[1]), self.mock.server_port)
        config['cloud'].update(agent_base_url=f'https://127.0.0.1:{self.rpc}', agent_trusted_ca_file=str(cert))
        config['agent_node'] = {'control_plane': {'bind_address': f'0.0.0.0:{self.rpc}', 'tls_certificate_file': str(cert),
            'tls_private_key_file': str(key), 'trusted_node_ca_file': str(cert)}}
        conf = self.home / 'talos.yaml'
        conf.write_text(yaml.safe_dump(config, sort_keys=False))
        env = {key: os.environ[key] for key in ('PATH', 'TMPDIR') if key in os.environ}
        env.update(HOME=str(self.home), TALOS_HOME=str(self.home), TALOS_CONFIG=str(conf), TALOS_RESOURCE_DIR=str(ROOT))
        self.log = (REPORT / 'output-logs-server.log').open('wb')
        self.server = subprocess.Popen([str(base.SERVER)], env=env, stdout=self.log, stderr=self.log)
        base.wait(lambda: base.http(self.url + '/health'), 'Real Server startup')
        for token in base.ACCOUNTS:
            base.data(base.http(self.url + '/v1/user', token=token))
        created = base.data(base.http(self.url + '/v1/computers', {
            'displayName': 'Output log acceptance', 'credentialExpiresAt': int(time.time()) + 3600}))
        self.node = created['node']['nodeId']
        fixture = RUN / 'worker-fixture'
        worker = base.config(fixture, base.free_port(), self.mock.server_port)
        worker['cloud'].update(api_base_url=f'http://host.docker.internal:{self.mock.server_port}',
            agent_base_url=f'https://host.docker.internal:{self.rpc}', agent_trusted_ca_file='/home/talos/.talos/conf/tls.crt')
        worker['llm']['openai']['base_url'] = f'http://host.docker.internal:{self.mock.server_port}/v1'
        worker['agent']['kernel']['tools']['bash']['output_log']['cleanup_interval_secs'] = 1
        worker['agent_node'] = {'runner': {'node_id': self.node, 'control_plane_endpoint': f'https://host.docker.internal:{self.rpc}',
            'credential_file': '/home/talos/.talos/conf/node.credential', 'trusted_ca_file': '/home/talos/.talos/conf/tls.crt', 'transport': 'outbound'}}
        (fixture / 'talos.yaml').write_text(yaml.safe_dump(worker, sort_keys=False))
        (fixture / 'node.credential').write_text(created['credential']['token'])
        (fixture / 'node.credential').chmod(0o600)
        (fixture / 'tls.crt').write_bytes(cert.read_bytes())
        docker_create('volume', self.volume, [])
        self.docker('run', '--rm', *container_args(self.name + '-init'), '--user', '0', '-v', self.volume + ':/home/talos/.talos',
            '-v', str(fixture) + ':/fixture:ro', IMAGE, '/bin/bash', '-ec',
            'mkdir -p /home/talos/.talos/conf; cp /fixture/* /home/talos/.talos/conf/; chown -R 10001:10001 /home/talos/.talos; chmod 700 /home/talos/.talos; chmod 600 /home/talos/.talos/conf/node.credential')
        self.start_worker()

    def start_worker(self):
        self.generation += 1
        self.docker('run', '-d', *container_args(self.name), '--add-host', 'host.docker.internal:host-gateway',
            '--security-opt', 'no-new-privileges:true', '-v', self.volume + ':/home/talos/.talos',
            '-e', 'TALOS_CONFIG=/home/talos/.talos/conf/talos.yaml', IMAGE)
        base.wait(lambda: base.data(base.http(self.url + '/v1/computers/' + self.node)).get('available'), 'Outbound Computer registration')

    def stop_worker(self, crash=False):
        self.docker('kill' if crash else 'stop', *(['--signal', 'KILL'] if crash else ['--time', '20']), self.name)
        state = json.loads(self.docker('inspect', self.name))[0]
        if not crash:
            base.require(state['State']['ExitCode'] == 0, 'Worker did not stop cleanly')
        logs = self.docker('logs', self.name)
        (REPORT / f'output-logs-worker-{self.generation}.log').write_text(logs)
        base.require('guard task started' in logs and 'bash_output_cleanup' in logs and 'output log maintenance run completed' in logs, 'Worker task lifecycle/run logs missing')
        if not crash:
            base.require('guard task stopped' in logs, 'Graceful shutdown did not join/log maintenance task')
        self.docker('rm', self.name)
        return state['Id']

    def turn(self, tool, args, session=None, wait=True, allow_error=False):
        session = session or 'sess_output_logs_' + uuid.uuid4().hex
        ack = base.data(base.http(self.url + '/v1/turns/start', {'sessionId': session, 'nodeId': self.node,
            'userPermissionMode': 'fullAccess', 'modelSelection': {'modelRef': {'provider': 'openai', 'id': base.MODEL['id']}, 'reasoningEffort': None},
            'input': {'content': [{'type': 'text', 'text': 'OUTPUT_LOG_TASK=' + json.dumps({'tool': tool, 'args': args})}]}}))
        if not wait:
            return session, ack
        request = base.urllib.request.Request(self.url + f'/v1/sessions/{session}/events?limit=10000&turn_id=' + ack['turnId'],
            headers={'Authorization': 'Bearer ' + base.TOKEN})
        events = []
        with base.urllib.request.urlopen(request, timeout=100) as response:
            for line in response:
                if not line.startswith(b'data: '):
                    continue
                record = json.loads(line[6:])
                if record.get('event', {}).get('type') != 'agent':
                    continue
                event = record['event']['data']
                events.append(event)
                if event['payload']['type'] in ('turnCompleted', 'turnFailed', 'turnCancelled'):
                    break
        (REPORT / (ack['turnId'] + '-events.json')).write_text(json.dumps(events))
        base.require(events and events[-1]['payload']['type'] == 'turnCompleted', 'Remote Turn failed: ' + str(events[-1] if events else ack))
        base.require('OUTPUT_LOG_ACCEPTANCE_OK' in json.dumps(events), 'Controlled model did not observe tool reply')
        results = [value for value in tool_values(events) if value.get('toolName') == tool and 'isError' in value]
        base.require(results, 'Requested tool did not complete')
        base.require(allow_error or not any(result['isError'] for result in results), 'Remote tool failed: ' + json.dumps(results)[:2000])
        return session, events

    def run(self):
        self.start()
        command = "cd /tmp && python3 -c \"import sys; [print('row-%06d '%i+'x'*90) for i in range(30000)]; print('REMOTE_FINAL_SENTINEL'); sys.exit(7)\""
        session, events = self.turn('Bash', {'command': command}, allow_error=True)
        log = next(value['outputLog'] for value in tool_values(events) if isinstance(value.get('outputLog'), dict))
        path = log['path']
        base.require(path.startswith(PREFIX + '/tool-output/'), 'Output belongs to wrong Workspace: ' + path)
        base.require(log['captureStatus'] == 'complete' and log['storedBytes'] > 2_000_000, 'Large output was not retained')
        output = next(value for value in tool_values(events) if value.get('outputLog') == log)
        base.require(output['exit_code'] == 7 and output['truncated'], 'Command failure or preview truncation was lost')
        mode = self.docker('exec', self.name, 'stat', '-c', '%u %a', path)
        base.require(mode == '10001 600', 'Log ownership or permissions are not private: ' + mode)
        self.snapshot_tree(path)
        base.require(not list(self.home.rglob('output.log')), 'Remote output incorrectly stored on Server')
        info = base.data(base.http(self.url + '/v1/computers/' + self.node))
        base.require(info['ownerUserId'] == base.USER['user_id'], 'Computer ownership mismatch')
        foreign = base.http(self.url + '/v1/computers/' + self.node, token='browser-contract-bob')
        base.require(foreign.get('status') != 'success', 'Another account discovered the private Computer')
        self.proof('remote_workspace_ownership', path=path, computer=self.node, volume=self.volume, storedBytes=log['storedBytes'])
        self.turn('Grep', {'path': path, 'pattern': 'REMOTE_FINAL_SENTINEL', 'output_mode': 'content'})
        base.require('REMOTE_FINAL_SENTINEL' in json.dumps(MODEL_REPLIES[-1]['replies']), 'Remote Grep missed sentinel')
        _, read = self.turn('Read', {'path': path, 'byte_offset': 0, 'max_bytes': 64}, session=session)
        first = next(value for value in tool_values(read) if value.get('nextByteOffset') is not None)
        cursor = first['nextByteOffset']
        old = self.stop_worker()
        self.start_worker()
        current = json.loads(self.docker('inspect', self.name))[0]['Id']
        base.require(current != old, 'Worker container was not replaced')
        _, read = self.turn('Read', {'path': path, 'byte_offset': cursor, 'max_bytes': 64}, session=session)
        base.require('row-' in json.dumps(MODEL_REPLIES[-1]['replies']), 'Original reference unreadable after replacement')
        self.proof('retained_reference_after_container_replacement', oldContainer=old, newContainer=current, cursor=cursor)
        # A normal spill setup error must not leak a full 64 MiB reservation.
        blocked_session = 'sess_output_logs_blocked_' + uuid.uuid4().hex
        blocked_path = PREFIX + '/tool-output/' + hashlib.sha256(blocked_session.encode()).hexdigest()
        self.volume_python('from pathlib import Path; import sys; Path(sys.argv[1]).write_text("preserve")', blocked_path)
        def quota_usage():
            return int(self.volume_python('from pathlib import Path; import json,sys; print(json.loads((Path(sys.argv[1])/"tool-output/quota.json").read_text())["used_bytes"])', PREFIX))
        quota_before = quota_usage()
        _, rejected = self.turn('Bash', {'command': "python3 -c \"print('rollback-test'*5000)\""}, session=blocked_session, allow_error=True)
        base.require(any(value.get('exit_code') == 0 for value in tool_values(rejected)), 'Spill failure lost actual command exit status')
        base.require(any(value.get('toolName') == 'Bash' and value.get('isError') for value in tool_values(rejected)), 'Spill setup failure was hidden')
        quota_after = quota_usage()
        base.require(quota_before == quota_after, f'Failed setup leaked quota: {quota_before} -> {quota_after}')
        self.volume_python('from pathlib import Path; import sys; Path(sys.argv[1]).unlink()', blocked_path)
        self.proof('failed_spill_setup_rolls_back_quota', beforeBytes=quota_before, afterBytes=quota_after, commandExitCode=0)
        before = {record['metadata'] for record in self.records()}
        self.turn('Bash', {'command': "python3 -u -c \"import time; print('INTERRUPTED_SENTINEL'); [print('active-'+str(i)+'x'*100) for i in range(3000)]; time.sleep(300)\""}, wait=False)
        active = base.wait(lambda: next((record for record in self.records() if record['metadata'] not in before and record['size'] and record['size'] > 100000), None), 'Live streamed spill')
        base.require(active['record']['log']['captureStatus'] == 'capturing', 'Capture finished before forced kill')
        requests = len(MODEL_REPLIES)
        time.sleep(2)
        base.require(len(MODEL_REPLIES) == requests, 'Idle maintenance unexpectedly invoked a model')
        live = next(record for record in self.records() if record['metadata'] == active['metadata'])
        base.require(live['record']['log']['captureStatus'] == 'capturing', 'Maintenance misclassified live capture')
        self.proof('concurrent_maintenance_preserves_active_capture', storedBytes=live['size'])
        self.stop_worker(crash=True)
        self.start_worker()
        recovered = base.wait(lambda: next((record for record in self.records() if record['metadata'] == active['metadata'] and record['record']['log']['captureStatus'] == 'interrupted'), None), 'Background orphan recovery without Bash')
        base.require(recovered['record']['log']['captureStatus'] == 'interrupted', 'Dead capture was not recovered')
        base.require(recovered['record']['log']['storedBytes'] == recovered['size'], 'Recovered size differs from retained bytes')
        self.turn('Read', {'path': recovered['record']['log']['path'], 'byte_offset': 0, 'max_bytes': 128})
        base.require('INTERRUPTED_SENTINEL' in json.dumps(MODEL_REPLIES[-1]['replies']), 'Interrupted output prefix lost')
        self.proof('sigkill_capture_recovery', storedBytes=recovered['size'], captureStatus='interrupted')
        # Create a terminal capture for live TTL and failure injection.
        # Deletion intent/body-removal injection below occurs only while stopped.
        self.turn('Bash', {'command': "python3 -c \"print('cleanup-test\\n'*100)\""})
        doomed = next(record for record in self.records() if record['metadata'] not in before and record['metadata'] != active['metadata'])
        # Verify periodic TTL and failure recovery without a new Bash invocation.
        requests = len(MODEL_REPLIES)
        self.volume_python('from pathlib import Path; import json,sys; p=Path(sys.argv[1]); r=json.loads(p.read_text()); r["log"]["expiresAt"]=1; p.write_text(json.dumps(r))', doomed['metadata'])
        base.wait(lambda: all(record['metadata'] != doomed['metadata'] for record in self.records()), 'Idle periodic TTL cleanup')
        base.require(len(MODEL_REPLIES) == requests, 'TTL cleanup depended on a tool invocation')
        self.proof('idle_periodic_expiry_without_bash', intervalSeconds=1)
        self.turn('Bash', {'command': "python3 -c \"print('deleting-test\\n'*100)\""})
        doomed = next(record for record in self.records() if record['metadata'] not in before and record['metadata'] != active['metadata'])
        saved = self.volume_python('from pathlib import Path; import sys; print(Path(sys.argv[1]).read_text())', doomed['metadata'])
        self.volume_python('from pathlib import Path; import sys; Path(sys.argv[1]).write_text("invalid metadata")', doomed['metadata'])
        base.wait(lambda: 'Bash log metadata is invalid.' in self.docker('logs', self.name), 'Cleanup failure diagnostic')
        failed_logs = self.docker('logs', self.name)
        self.volume_python('from pathlib import Path; import sys; Path(sys.argv[1]).write_text(sys.argv[2])', doomed['metadata'], saved)
        base.wait(lambda: any('maintenance run completed' in line and 'failed_workspaces=0' in line for line in self.docker('logs', self.name)[len(failed_logs):].splitlines()), 'Next periodic round recovers without Worker restart')
        self.proof('maintenance_failure_is_logged_and_recovers')
        self.stop_worker()
        self.volume_python('from pathlib import Path; import json,sys; '
            'p=Path(sys.argv[1]); r=json.loads(p.read_text()); r["cleanup"]="deleting"; p.write_text(json.dumps(r)); (p.parent/"output.log").unlink(); '
            'p=Path(sys.argv[2]); r=json.loads(p.read_text()); r["log"]["expiresAt"]=1; p.write_text(json.dumps(r)); '
            'p=Path(sys.argv[3])/"tool-output/foreign/data/keep"; p.mkdir(parents=True); (p/"unrelated.txt").write_text("preserve")',
            doomed['metadata'], active['metadata'], PREFIX)
        self.start_worker()
        base.wait(lambda: all(record['metadata'] not in (doomed['metadata'], active['metadata']) for record in self.records()), 'Startup cleanup without Bash')
        remaining = self.records()
        base.require(all(record['metadata'] not in (doomed['metadata'], active['metadata']) for record in remaining), 'Cleanup did not resume')
        base.require(any(record['record']['log']['path'] == path for record in remaining), 'Retained log was incorrectly removed')
        base.require(self.volume_python('from pathlib import Path; import sys; print((Path(sys.argv[1])/"tool-output/foreign/data/keep/unrelated.txt").read_text())', PREFIX) == 'preserve', 'Cleanup removed unknown data')
        self.turn('Read', {'path': path, 'offset': 30001, 'limit': 1})
        base.require('REMOTE_FINAL_SENTINEL' in json.dumps(MODEL_REPLIES[-1]['replies']), 'Retained log unreadable after cleanup')
        self.proof('cleanup_recovery_and_expiry', deletionIntentResumed=True, expiredRemoved=True, retainedReadable=True, foreignPreserved=True)
        (REPORT / 'output-logs-model-replies.json').write_text(json.dumps(MODEL_REPLIES, indent=2))
        logs = self.docker('logs', self.name)
        base.require('reason=\"periodic\"' in logs or 'reason=periodic' in logs, 'Periodic execution log missing')
        self.stop_worker()
        self.proof('maintenance_task_lifecycle_logs', gracefulStopsVerified=True)
        self.full_round_acceptance(path)
        (REPORT / 'output-logs-model-replies.json').write_text(json.dumps(MODEL_REPLIES, indent=2))

    def full_round_acceptance(self, retained_path):
        """Seed a finite >4096-entry volume while stopped; hourly interval means
        all cleanup must finish in the startup round, without a second tick."""
        summary = json.loads(self.volume_python(r"""
from pathlib import Path
import json, os, sys
workspace = Path(sys.argv[1]); prefix = workspace.parent
conf = Path('/home/talos/.talos/conf/talos.yaml')
# Keep this fixture dependency-free inside the Worker image.
text = conf.read_text(); text = text.replace('cleanup_interval_secs: 1\n', 'cleanup_interval_secs: 3600\n'); conf.write_text(text)
assert 'cleanup_interval_secs: 3600' in text
expected = 0
for user, count in [(workspace.name, 4208), ('offline-output-user', 2)]:
    ws = prefix/user; root = ws/'tool-output'; turn = root/'full-round'/'turn'
    turn.mkdir(parents=True, exist_ok=True)
    for index in range(count):
        directory = turn/str(index); directory.mkdir()
        path = directory/'output.log'; path.write_text('retained output')
        expired = index % 37 == 0
        expected += int(expired)
        record = dict(version=1, reservation_bytes=4096+15, cleanup='retained', log=dict(
            path=str(path), relativePath=str(path.relative_to(ws)), totalBytes=15, storedBytes=15,
            totalLines=1, captureStatus='complete', expiresAt=1 if expired else 9223372036854775807))
        (directory/'metadata.json').write_text(json.dumps(record))
    if user == workspace.name:
        broken = turn/'broken'; broken.mkdir(); (broken/'metadata.json').write_text('invalid metadata')
        foreign = turn/'unknown'; foreign.mkdir(); (foreign/'unrelated.txt').write_text('preserve')
        ledger = root/'quota.json'; old = json.loads(ledger.read_text())
        charged = sum(json.loads(p.read_text())['reservation_bytes'] for p in root.glob('*/*/*/metadata.json') if p.parent.name != 'broken')
        # Broken ownership remains charged conservatively until repaired.
        ledger.write_text(json.dumps(dict(version=2, used_bytes=charged+4096, revision=old['revision']+1)))
    for path in [ws, *ws.rglob('*')]:
        os.chown(path, 10001, 10001); os.chmod(path, 0o700 if path.is_dir() else 0o600)
print(json.dumps(dict(seedLogs=4210, expectedExpired=expected, intervalSeconds=3600)))
""", PREFIX))
        before = len(MODEL_REPLIES)
        self.start_worker()
        logs = base.wait(lambda: (value if 'reason="startup"' in value and 'output log maintenance run completed' in value else None)
                         if (value := self.docker('logs', self.name)) else None, 'Complete startup round beyond 4096 entries', seconds=120)
        counts = json.loads(self.volume_python("""
from pathlib import Path
import json, sys
ws = Path(sys.argv[1]); result = dict(expired=0, retained=0, ledgers={})
for user in [ws.name, 'offline-output-user']:
    root = ws.parent/user/'tool-output'
    for p in root.glob('full-round/turn/*/metadata.json'):
        if p.parent.name == 'broken': continue
        record = json.loads(p.read_text()); result['expired' if record['log']['expiresAt'] == 1 else 'retained'] += 1
    result['ledgers'][user] = json.loads((root/'quota.json').read_text())
assert (ws/'tool-output/full-round/turn/broken/metadata.json').read_text() == 'invalid metadata'
assert (ws/'tool-output/full-round/turn/unknown/unrelated.txt').read_text() == 'preserve'
print(json.dumps(result))
""", PREFIX))
        base.require(counts['expired'] == 0 and counts['retained'] == summary['seedLogs']-summary['expectedExpired'], 'Startup sweep skipped expired logs or evicted unexpired logs: '+str(counts))
        base.require(len(MODEL_REPLIES) == before, 'Full sweep required a model/tool invocation')
        base.require('failed_workspaces=1' in logs, 'Corrupt metadata was not isolated and reported')
        base.require('accounting reconciliation deferred' in logs, 'Corrupt accounting audit was not deferred')
        self.proof('full_startup_round_crosses_4096_and_rotates_users', **summary, **counts)
        # Restart with repaired ownership, forcing complete ledger recovery.
        self.stop_worker()
        self.volume_python("from pathlib import Path; import sys; root=Path(sys.argv[1])/'tool-output'; (root/'full-round/turn/broken/metadata.json').unlink(); (root/'quota.json').unlink()", PREFIX)
        self.start_worker()
        base.wait(lambda: 'output log maintenance run completed' in self.docker('logs', self.name), 'Recovery startup round')
        recovered = json.loads(self.volume_python("""
from pathlib import Path
import json, sys
root=Path(sys.argv[1])/'tool-output'; ledger=json.loads((root/'quota.json').read_text())
charged=sum(json.loads(p.read_text())['reservation_bytes'] for p in root.glob('*/*/*/metadata.json'))
assert ledger['version']==2 and ledger['used_bytes']==charged, (ledger,charged)
print(json.dumps(dict(ledger=ledger, metadataCharge=charged)))
""", PREFIX))
        self.turn('Read', {'path': retained_path, 'byte_offset': 0, 'max_bytes': 64})
        base.require('row-' in json.dumps(MODEL_REPLIES[-1]['replies']), 'Old large output reference unreadable after full sweep/restart')
        self.proof('full_round_restart_recovers_ledger_and_old_reference', **recovered)
        self.stop_worker()

    def close(self):
        diagnostic = subprocess.run(['docker', 'logs', self.name], capture_output=True, text=True)
        (REPORT / f'output-logs-worker-{self.generation}-final.log').write_text(diagnostic.stdout + diagnostic.stderr)
        subprocess.run(['docker', 'rm', '-f', self.name], capture_output=True)
        subprocess.run(['docker', 'volume', 'rm', self.volume], capture_output=True)
        if self.server:
            self.server.send_signal(signal.SIGINT)
            try:
                self.server.wait(timeout=20)
            except subprocess.TimeoutExpired:
                self.server.kill()
                self.server.wait(timeout=5)
            self.log.close()
        self.mock.shutdown()
        self.mock.server_close()


if __name__ == '__main__':
    acceptance = Acceptance()
    try:
        acceptance.run()
    finally:
        acceptance.close()
