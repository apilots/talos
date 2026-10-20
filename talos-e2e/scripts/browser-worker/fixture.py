"""Secretless real Server -> outbound Docker Worker -> built-in MCP acceptance."""
import copy
import re
import base64
import json
import os
from pathlib import Path
import signal
import socket
import sqlite3
import subprocess
import threading
import time
import urllib.request
import urllib.error
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import yaml

ROOT = Path(__file__).resolve().parents[3]
REPORT = Path(os.environ.get('TALOS_SCENARIO_REPORT_DIR', str(ROOT / 'output/browser-worker')))
RUN = ROOT / '.tmp/browser-worker/task-run' / uuid.uuid4().hex
REPORT.mkdir(parents=True,exist_ok=True)
RUN.mkdir(parents=True, exist_ok=True)
IMAGE=os.environ.get('TALOS_E2E_WORKER_IMAGE','talos-worker:browser-e2e')
SERVER=Path(os.environ.get('TALOS_E2E_SERVER_BIN',str(ROOT/'talos-rs/target/debug/talos-server')))
TOKEN = 'browser-contract-alice'
USER = {'user_id': 'browser-alice', 'email': 'alice@example.test', 'username': 'alice',
        'role': 'user', 'type': 'free', 'status': 'active', 'email_verified': True,
        'set_password_required': False, 'disabled': False, 'created_at_ms': 1, 'updated_at_ms': 1}
MODEL = {'id': 'browser-fixture', 'name': 'Browser fixture', 'context_window': 131072,
         'reasoning': False, 'reasoning_effort': 'medium', 'speed': 'standard', 'input': ['text', 'image'],
         'cost': {'input': 0.0, 'output': 0.0, 'cache_read': 0.0, 'cache_write': 0.0}}
calls = []
device_logins = {}
SESSION_TITLE = 'Browser Worker acceptance'

def auxiliary_response(request):
    """Handle only the two product auxiliary workloads used by this scenario."""
    messages = request.get('messages', [])
    if not isinstance(messages, list) or len(messages) != 2 or not all(isinstance(m, dict) for m in messages):
        raise ValueError('Unsupported auxiliary message shape')
    if [m.get('role') for m in messages] != ['system', 'user']:
        raise ValueError('Unsupported auxiliary message shape')
    prompt = messages[0].get('content', '')
    payload = json.loads(messages[1]['content'])
    if not isinstance(prompt, str) or not isinstance(payload, dict):
        raise ValueError('Unsupported auxiliary content shape')
    if prompt.startswith('Generate a concise session title for the conversation.'):
        if set(payload) == {'user_message', 'assistant_response'}:
            return SESSION_TITLE
    elif prompt.startswith('Decide whether this completed primary-agent Turn contains durable information'):
        if set(payload) == {'currentMemory', 'userMessage', 'assistantResponse'}:
            return json.dumps({'operation': 'noop'})
    raise ValueError('Unsupported auxiliary workload')

def require(value, reason):
    if not value:
        raise AssertionError(reason)

def free_port():
    with socket.socket() as sock:
        sock.bind(('127.0.0.1', 0))
        return sock.getsockname()[1]

def http(url, body=None, token=TOKEN):
    req = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(),
        headers={'Content-Type': 'application/json', 'Authorization': 'Bearer ' + token})
    try:
        response = urllib.request.build_opener(urllib.request.ProxyHandler({})).open(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return json.load(response)

def data(response):
    require(response.get('status') == 'success', 'Public API failed: ' + str(response.get('error')))
    return response['data']

def wait(probe, description, seconds=45):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except (OSError, ValueError):
            pass
        time.sleep(.2)
    raise AssertionError(description + ' timed out')

def find_elements(value):
    if isinstance(value, dict):
        if 'elements' in value and isinstance(value['elements'], list):
            return value['elements']
        for child in value.values():
            result = find_elements(child)
            if result:
                return result
    elif isinstance(value, list):
        for child in value:
            result = find_elements(child)
            if result:
                return result
    elif isinstance(value, str):
        for pos, char in enumerate(value):
            if char == '{':
                try:
                    result = find_elements(json.JSONDecoder().raw_decode(value[pos:])[0])
                    if result:
                        return result
                except ValueError:
                    pass
    return []

class Fixture(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'
    def log_message(self, *_):
        pass
    def reply(self, value, status=200):
        raw = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.send_header('Access-Control-Allow-Origin','*')
        self.end_headers()
        self.wfile.write(raw)
    def do_GET(self):
        if self.path == '/v1/auth/config':
            return self.reply({'registration_enabled': True})
        if self.path == '/v1/me':
            return self.reply(USER)
        if self.path.startswith('/device'):
            return self.reply({'status':'approved','account':'secretless browser acceptance'})
        self.reply({'error': 'not_found'}, 404)
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path == '/v1/auth/device/start':
            session=str(uuid.uuid4());code=str(uuid.uuid4());device_logins[session]=code
            verification=f'http://127.0.0.1:{self.server.server_port}/device'
            return self.reply({'session_id':session,'device_code':code,'user_code':'BROWSER-TEST','verification_uri':verification,
                'verification_uri_complete':verification+'?session_id='+session,'expires_at_ms':int(time.time()*1000)+60000,'interval_ms':100})
        if self.path == '/v1/auth/device/poll':
            require(device_logins.get(request.get('session_id'))==request.get('device_code'),'Unknown fixture device login')
            return self.reply({'status':'approved','auth':{'access_token':TOKEN,'token_type':'Bearer','expires_at_ms':int(time.time()*1000)+3600000,
                'user':USER,'capabilities':{'is_admin':False,'role':'user','scopes':['talos:runtime']}}})
        if self.path == '/v1/auth/device/cancel':
            return self.reply({'status':'expired'})
        if self.path == '/v1/auth/logout':
            return self.reply({'success':True})
        if self.path == '/v1/auth/introspect':
            active = request.get('token') in (TOKEN,'browser-contract-bob')
            user=dict(USER)
            if request.get('token')=='browser-contract-bob':user.update(user_id='browser-bob',username='bob',email='bob@example.test')
            return self.reply({'active': active, 'user': user if active else None, 'capabilities': None,
                'entitlement': {'can_use': True, 'plan': 'free', 'reason': None} if active else None, 'expires_at_ms': None})
        if self.path != '/v1/chat/completions':
            return self.reply({'error': 'not_found'}, 404)
        names = [t['function']['name'] for t in request.get('tools', [])]
        if not names:
            try:
                content = auxiliary_response(request)
            except (ValueError, KeyError, TypeError):
                return self.reply({'error': {'message': 'Unsupported auxiliary workload', 'type': 'invalid_request_error'}}, 400)
            return self.reply({'id': 'chat_auxiliary', 'object': 'chat.completion', 'created': int(time.time()), 'model': 'browser-fixture',
                'choices': [{'index': 0, 'message': {'role': 'assistant', 'content': content}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}})
        tools = [m for m in request['messages'] if m['role'] == 'tool']
        step = sum(call.get('function',{}).get('name','').lower() != 'toolsearch'
                   for message in request['messages'] for call in message.get('tool_calls',[]))
        name, args = None, {}
        file_match=re.search(r'BROWSER_UPLOAD_FILE_ID=([a-zA-Z0-9_-]+)',json.dumps(request['messages']))
        sequence = ['browser_navigate', 'browser_state', 'browser_fill', 'browser_state', 'browser_click', 'browser_state', 'browser_screenshot']
        if 'BROWSER_KEEP_HOME' in json.dumps(request['messages']):
            sequence.insert(0, 'browser_tab')
        if file_match:
            sequence += ['browser_state','browser_upload','browser_upload','browser_state','browser_click','browser_download']
        if not any('browser_navigate' in n for n in names):
            name = next((n for n in names if n.lower() == 'toolsearch'), None)
            args = {'query': 'browser', 'limit': 10}
        else:
            if step < len(sequence):
                suffix = sequence[step]
                name = next((n for n in names if n.endswith(suffix)), None)
                if suffix == 'browser_navigate':
                    args = {'url': 'http://127.0.0.1:18080/'}
                elif suffix == 'browser_tab':
                    args = {'action': 'open', 'url': 'about:blank'}
                elif suffix in ('browser_fill', 'browser_click', 'browser_upload'):
                    elements=next((found for item in reversed(tools) if (found:=find_elements(item['content']))),[])
                    tag = 'input' if suffix in ('browser_fill','browser_upload') else ('a' if file_match and step>sequence.index('browser_upload') else 'button')
                    chosen = next((e for e in elements if e['tag'] == tag and (suffix!='browser_upload' or e.get('attributes',{}).get('type')=='file')), None)
                    require(chosen, 'State did not expose fixture ' + tag)
                    args = {'element': chosen['element']}
                    if suffix == 'browser_fill':
                        args['text'] = 'Worker'
                    elif suffix == 'browser_upload':
                        args['fileId']='not-attached-to-this-turn' if step==sequence.index('browser_upload') else file_match.group(1)
        if step < len(sequence):
            require(name, 'Requested browser tool absent; catalog=' + repr(names))
            calls.append('ToolSearch' if name.lower() == 'toolsearch' else name)
            print('MODEL_STEP', len(calls), name, flush=True)
            delta = {'role': 'assistant', 'tool_calls': [{'index': 0, 'id': 'call_' + uuid.uuid4().hex,
                'type': 'function', 'function': {'name': name, 'arguments': json.dumps(args)}}]}
            finish = 'tool_calls'
        else:
            require(any('Hello Worker' in str(m['content']) for m in tools), 'Fixture mutation was not observed by actual MCP state')
            if 'BROWSER_VERIFY_COOKIE' in json.dumps(request['messages']):
                require(any('acceptance=retained' in str(m['content']) for m in tools),'Profile cookie was not retained after replacement')
            if file_match:
                require(any('report.csv' in str(m['content']) for m in tools),'Completed browser download was not persisted')
                (REPORT/'file-tool-model-replies.json').write_text(json.dumps(tools[sequence.index('browser_upload')-1:],indent=2))
                require(any('permission_rejection' in str(m['content']) for m in tools),'Unauthorized attachment was not rejected')
                require(any('browser-upload.png' in str(m['content']) for m in tools),'Authorized attachment did not reach the browser')
            delta, finish = {'role': 'assistant', 'content': 'BROWSER_WORKER_ACCEPTANCE_OK'}, 'stop'
        if request.get('stream'):
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Connection', 'close')
            self.end_headers()
            for item in [delta, {}]:
                value = {'id': 'chat_fixture', 'object': 'chat.completion.chunk', 'created': int(time.time()),
                    'model': 'browser-fixture', 'choices': [{'index': 0, 'delta': item, 'finish_reason': finish if not item else None}]}
                self.wfile.write(('data: ' + json.dumps(value) + '\n\n').encode())
            self.wfile.write(b'data: [DONE]\n\n')
            self.wfile.flush()
            self.close_connection = True
        else:
            delta.pop('index', None)
            for call in delta.get('tool_calls', []):
                call.pop('index', None)
            self.reply({'id': 'chat_fixture', 'object': 'chat.completion', 'created': int(time.time()), 'model': 'browser-fixture',
                'choices': [{'index': 0, 'message': delta, 'finish_reason': finish}], 'usage': {'prompt_tokens': 1, 'completion_tokens': 1, 'total_tokens': 2}})

def config(home, http_port, mock_port):
    document = yaml.safe_load((ROOT / 'conf/talos-dev.yaml').read_text())
    document['gateway'].update(port=http_port, host='127.0.0.1', auth=True)
    document['cloud'].update(api_base_url=f'http://127.0.0.1:{mock_port}', web_base_url=f'http://127.0.0.1:{mock_port}')
    document['llm'].update(provider='openai', model='browser-fixture')
    for key, provider in document['llm'].items():
        if isinstance(provider, dict) and 'models' in provider:
            provider.update(models=[], api_key=None)
    document['llm']['openai'].update(models=[MODEL], api_key='fixture', base_url=f'http://127.0.0.1:{mock_port}/v1', protocol='chat')
    document['agent']['inner_model'] = {'provider': 'openai', 'id': 'browser-fixture'}
    document['agent']['kernel']['plugins'].update(sources=[], roots=[])
    document['agent']['kernel']['mcp']['servers'] = []
    document['sync']['enabled'] = False
    home.mkdir(parents=True, exist_ok=True)
    (home / '.env').write_text('')
    return document

def browser_task(url,node,attachment,label,verify_cookie=False):
    calls.clear()
    session = 'sess_browser_' + uuid.uuid4().hex
    ack = data(http(url + '/v1/turns/start', {'sessionId': session, 'nodeId': node, 'userPermissionMode': 'fullAccess',
        'modelSelection': {'modelRef': {'provider': 'openai', 'id': 'browser-fixture'}, 'reasoningEffort': None},
        'input': {'content': [{'type': 'text', 'text': 'BROWSER_WORKER_ACCEPTANCE BROWSER_KEEP_HOME '+('BROWSER_VERIFY_COOKIE ' if verify_cookie else '')+'BROWSER_UPLOAD_FILE_ID='+attachment},{'type':'file','fileId':attachment}]}}))
    req = urllib.request.Request(url + f'/v1/sessions/{session}/events?limit=10000&turn_id=' + ack['turnId'], headers={'Authorization': 'Bearer ' + TOKEN})
    events = []
    with urllib.request.urlopen(req, timeout=150) as response:
        for line in response:
            if line.startswith(b'data: '):
                record = json.loads(line[6:])
                if record.get('event', {}).get('type') != 'agent':
                    continue
                event = record['event']['data']; events.append(event)
                if event.get('payload', {}).get('type') in ('turnCompleted', 'turnFailed', 'turnCancelled'):
                    break
    (REPORT / (label+'-events.json')).write_text(json.dumps(events))
    require(events and events[-1]['payload']['type'] == 'turnCompleted', 'Turn did not complete: ' + str(events[-1]['payload'] if events else 'no events'))
    # Title generation must commit before the successful terminal fact reaches clients.
    completed = data(http(url + '/v1/sessions/' + session))['session']
    listed = data(http(url + '/v1/sessions'))['sessions']
    require(completed['title'] == SESSION_TITLE, 'Completed Session detail has an invalid generated title')
    require(any(item['id'] == session and item['title'] == SESSION_TITLE for item in listed),
            'Completed Session list does not expose its generated title')
    require('BROWSER_WORKER_ACCEPTANCE_OK' in json.dumps(events), 'Fixture did not finish the browser task')
    require(len(calls) >= 7, 'Browser task skipped actions')
    results = [e['payload']['data']['block']['content']['data'] for e in events
        if e['payload']['type'] == 'messageBlockCompleted' and e['payload']['data']['block']['content']['type'] == 'toolResult']
    require(len(results)==14,'A real browser tool was skipped')
    require(sum(result['isError'] for result in results)==1,'Expected only the forged attachment rejection')
    from home import is_home_url
    opened = next(c['value'] for c in results[0]['content'] if c['type']=='json' and 'targetId' in c.get('value',{}))
    first_state = next(c['value'] for c in results[2]['content'] if c['type']=='json' and 'tabs' in c.get('value',{}))
    require(opened['url']=='about:blank' and first_state['targetId']==opened['targetId'],
            'Browser task did not navigate its newly opened tab')
    require(any(is_home_url(tab['url']) and tab['tabId']!=opened['targetId'] for tab in first_state['tabs']),
            'Browser Use closed the packaged background homepage')
    images = [c for result in results for c in result['content'] if c['type'] == 'image']
    require(len(images) == 1 and images[0].get('fileId'), 'Screenshot did not use the original file callback')
    file_id = images[0]['fileId']
    image = data(http(url + '/v1/files/' + file_id))
    require(image['file']['mimeType'] == 'image/png', 'Stored screenshot MIME mismatch')
    content = data(http(url + '/v1/files/' + file_id + '/content'))
    png = base64.b64decode(content['dataBase64'], validate=True)
    require(png.startswith(b'\x89PNG\r\n\x1a\n'), 'Screenshot public download is not PNG')
    (REPORT / (label+'-screenshot.png')).write_bytes(png)
    require('data:image' not in json.dumps(events) and 'base64' not in json.dumps(events), 'Timeline persisted inline image bytes')
    return session,events,results

def native_media(document,node,server_home):
    home=RUN/('desktop-media-'+uuid.uuid4().hex[:8]);home.mkdir()
    (home/'data').mkdir()
    # Seed the owned directory snapshot; G/H routing and media authorization remain real.
    with sqlite3.connect(server_home/'data/talos.db') as source, sqlite3.connect(home/'data/talos.db') as target:source.backup(target)
    desktop=copy.deepcopy(document);desktop['agent_node']={}
    (home/'conf').mkdir();conf=home/'conf/talos.yaml';conf.write_text(json.dumps(desktop))
    (home/'auth').mkdir();(home/'auth/cloud-session.json').write_text(json.dumps({'schema_version':2,'cloud_api_base_url':desktop['cloud']['api_base_url'],'access_token':TOKEN,'token_type':'Bearer','expires_at_ms':int(time.time()*1000)+3600000,'last_used_at_ms':int(time.time()*1000),'user':USER}))
    (home/'auth/cloud-session.json').chmod(0o600)
    env={key:os.environ[key] for key in ('PATH','TMPDIR','DISPLAY','XDG_RUNTIME_DIR') if key in os.environ}
    env.update(HOME=str(home),TALOS_HOME=str(home),TALOS_CONFIG=str(conf),TALOS_RESOURCE_DIR=str(ROOT),TALOS_APP_SERVER_BIN=os.environ.get('TALOS_E2E_APP_SERVER_BIN',str(ROOT/'talos-rs/target/debug/app-server')),TALOS_E2E_REMOTE_NODE=node,TALOS_SCENARIO_REPORT_DIR=str(REPORT))
    electron=ROOT/'talos-app/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
    if not electron.exists():electron=ROOT/'talos-app/desktop/node_modules/electron/dist/electron'
    with (REPORT/'native-docker-media.log').open('wb') as log:
        subprocess.run([str(electron),str(ROOT/'.tmp/browser-worker/native/media-dist')],env=env,stdout=log,stderr=log,timeout=100,check=True)
    require('NATIVE_DOCKER_MEDIA_ACCEPTANCE_PASS' in (REPORT/'native-docker-media.log').read_text(),'Desktop viewer exited without completing acceptance')

def run():
    mock = ThreadingHTTPServer(('0.0.0.0', 0), Fixture)
    print('FIXTURE_PORT', mock.server_port, flush=True)
    threading.Thread(target=mock.serve_forever, daemon=True).start()
    http_port, rpc_port = free_port(), free_port()
    home = RUN / ('server-' + uuid.uuid4().hex[:8])
    cert, key = RUN / 'tls.crt', RUN / 'tls.key'
    subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-days', '1', '-subj', '/CN=host.docker.internal',
        '-addext', 'subjectAltName=DNS:host.docker.internal,IP:127.0.0.1', '-addext', 'basicConstraints=critical,CA:FALSE',
        '-addext', 'keyUsage=critical,digitalSignature,keyEncipherment', '-addext', 'extendedKeyUsage=serverAuth',
        '-keyout', str(key), '-out', str(cert)], check=True, capture_output=True)
    document = config(home, http_port, mock.server_port)
    document['cloud'].update(agent_base_url=f'https://127.0.0.1:{rpc_port}', agent_trusted_ca_file=str(cert))
    document['agent_node'] = {'control_plane': {'bind_address': f'0.0.0.0:{rpc_port}', 'tls_certificate_file': str(cert),
        'tls_private_key_file': str(key), 'trusted_node_ca_file': str(cert)}}
    conf = home / 'talos.yaml'
    conf.write_text(yaml.safe_dump(document, sort_keys=False))
    env = {k: os.environ[k] for k in ('PATH', 'TMPDIR', 'SYSTEMROOT') if k in os.environ}
    env.update(HOME=str(home), TALOS_HOME=str(home), TALOS_CONFIG=str(conf), TALOS_RESOURCE_DIR=str(ROOT))
    env['TALOS_WEBUI_DIR']=os.environ.get('TALOS_WEBUI_DIR',str(ROOT/'talos-app/web/dist'))
    log = (REPORT / 'task-server.log').open('wb')
    server = subprocess.Popen([str(SERVER)], env=env, stdout=log, stderr=log)
    name = 'talos-browser-task-' + uuid.uuid4().hex[:8]
    volume = name + '-home'
    try:
        url = f'http://127.0.0.1:{http_port}'
        print('SERVER_PORT', http_port, 'AGENT_PORT', rpc_port, flush=True)
        wait(lambda: http(url + '/health'), 'Server startup')
        created = data(http(url + '/v1/computers', {'displayName': 'Browser acceptance', 'credentialExpiresAt': int(time.time()) + 3600}))
        node = created['node']['nodeId']
        worker = config(RUN / 'worker-fixture', http_port, mock.server_port)
        worker['cloud'].update(api_base_url=f'http://host.docker.internal:{mock.server_port}',
            agent_base_url=f'https://host.docker.internal:{rpc_port}', agent_trusted_ca_file='/home/talos/.talos/conf/tls.crt')
        worker['llm']['openai']['base_url'] = f'http://host.docker.internal:{mock.server_port}/v1'
        worker['agent_node'] = {'runner': {'node_id': node, 'control_plane_endpoint': f'https://host.docker.internal:{rpc_port}',
            'credential_file': '/home/talos/.talos/conf/node.credential', 'trusted_ca_file': '/home/talos/.talos/conf/tls.crt', 'transport': 'outbound'}}
        fixture = RUN / 'worker-fixture'
        (fixture / 'talos.yaml').write_text(yaml.safe_dump(worker, sort_keys=False))
        (fixture / 'node.credential').write_text(created['credential']['token'])
        (fixture / 'node.credential').chmod(0o600)
        (fixture / 'tls.crt').write_bytes(cert.read_bytes())
        (fixture / 'website.py').write_text('''from http.server import BaseHTTPRequestHandler,HTTPServer
class H(BaseHTTPRequestHandler):
 def do_GET(self):
  if self.path=='/report.csv':
   self.send_response(200);self.send_header('Content-Type','text/csv');self.send_header('Content-Disposition','attachment; filename=\"report.csv\"');self.end_headers();self.wfile.write(b'name,value\\nworker,1\\n');return
  b=b"""<title>Worker fixture</title><input name="name"><button onclick="document.querySelector('p').textContent='Hello '+document.querySelector('input').value;document.cookie='acceptance=retained;max-age=3600'">Submit</button><p>Ready</p><output id="cookie"></output><script>document.querySelector('#cookie').textContent=document.cookie</script><input id="upload" type="file" onchange="document.querySelector('#uploaded').textContent=this.files[0].name"><p id="uploaded"></p><a href="/report.csv">Download</a>"""
  self.send_response(200);self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
 def log_message(self,*a): pass
HTTPServer(('127.0.0.1',18080),H).serve_forever()
''')
        subprocess.run(['docker', 'volume', 'create', volume], check=True, capture_output=True)
        subprocess.run(['docker', 'run', '--rm', '--user', '0', '-v', volume + ':/home/talos/.talos', '-v', str(fixture) + ':/fixture:ro',
            IMAGE, '/bin/bash', '-ec', 'mkdir -p /home/talos/.talos/conf; cp /fixture/* /home/talos/.talos/conf/; chown -R 10001:10001 /home/talos/.talos; chmod 700 /home/talos/.talos; chmod 600 /home/talos/.talos/conf/node.credential'], check=True, capture_output=True)
        worker_command=['docker', 'run', '-d', '--name', name, '--add-host','host.docker.internal:host-gateway', '--shm-size=1g', '--security-opt', 'no-new-privileges:true', '--security-opt',
            'seccomp=' + str(ROOT / 'deploy/docker/talos-worker/chromium-seccomp.json'), '-v', volume + ':/home/talos/.talos',
            '-e', 'TALOS_CONFIG=/home/talos/.talos/conf/talos.yaml', IMAGE, '/bin/bash', '-ec',
            'python3 /home/talos/.talos/conf/website.py & exec /opt/talos/bin/talos-worker']
        subprocess.run(worker_command,check=True,capture_output=True)
        wait(lambda: data(http(url + '/v1/computers/' + node)).get('available'), 'Outbound Worker registration')
        snapshot = data(http(url + '/v1/browser', {'nodeId': node, 'operation': {'kind': 'read'}}))
        require(snapshot['host'] == 'docker' and snapshot['enabled'], 'Wrong canonical browser capability')
        from home import desktop_home
        desktop_home(url,node,name,http,data,wait,REPORT)
        png=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a5n8AAAAASUVORK5CYII=')
        boundary='talos-browser-'+uuid.uuid4().hex
        metadata=json.dumps({'filename':'browser-upload.png','scope':{'type':'personal'},'tags':[],'metadata':{}})
        body=('--'+boundary+'\r\nContent-Disposition: form-data; name="metadata"\r\nContent-Type: application/json\r\n\r\n'+metadata+'\r\n--'+boundary+'\r\nContent-Disposition: form-data; name="file"; filename="browser-upload.png"\r\nContent-Type: image/png\r\n\r\n').encode()+png+('\r\n--'+boundary+'--\r\n').encode()
        upload_request=urllib.request.Request(url+'/v1/files',data=body,headers={'Authorization':'Bearer '+TOKEN,'Content-Type':'multipart/form-data; boundary='+boundary})
        with urllib.request.urlopen(upload_request,timeout=10) as response:attachment=data(json.load(response))['file']['fileId']
        session,events,results=browser_task(url,node,attachment,'task')
        inspected = json.loads(subprocess.check_output(['docker', 'inspect', name]))[0]
        require(not inspected['HostConfig']['PortBindings'], 'Worker unexpectedly published a port')
        listeners = subprocess.check_output(['docker', 'exec', name, 'cat', '/proc/net/tcp', '/proc/net/tcp6']).decode()
        (REPORT / 'task-private-listeners.txt').write_text(listeners)
        entries = [line.split()[1] for line in listeners.splitlines() if len(line.split()) > 3 and line.split()[3] == '0A']
        require(all(entry.split(':')[0] in ('0100007F', '00000000000000000000000001000000') for entry in entries), 'Worker opened a non-loopback TCP listener')
        print('PASS real outbound Docker Worker / trusted ToolContext / built-in MCP / browser actions / screenshot / authorized files', flush=True)
        from media import media,pressure_and_expiry
        media(url,node,name,http,data,wait,REPORT)
        if os.environ.get('TALOS_E2E_NATIVE_MEDIA')=='1':native_media(document,node,home)
        pressure_and_expiry(url,node,http,data,wait,REPORT)
        before=data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'read'}}))
        data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'pause','expectedRevision':before['control']['revision']}}))
        subprocess.run(['docker','stop','--time','15',name],check=True,capture_output=True)
        stopped=json.loads(subprocess.check_output(['docker','inspect',name]))[0]
        require(stopped['State']['ExitCode']==0,'Worker did not stop cleanly on SIGTERM')
        subprocess.run(['docker','rm',name],check=True,capture_output=True)
        subprocess.run(worker_command,check=True,capture_output=True)
        def replaced():
            response=http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'read'}})
            if response.get('error',{}).get('code')=='serviceUnavailable':return None
            value=data(response)
            return value if value['control']['generation']!=before['control']['generation'] else None
        after=wait(replaced,'Replacement Worker registration')
        require(after['control']['owner']['kind']=='suspended','Replacement lost persisted manual pause')
        data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'resume','expectedRevision':after['control']['revision']}}))
        browser_task(url,node,attachment,'replacement-task',verify_cookie=True)
        (REPORT/'profile-replacement-acceptance.json').write_text(json.dumps({'status':'passed','newGeneration':True,'pausePersisted':True,'cookieRetained':True,'cleanSigterm':True}))
        print('PASS clean SIGTERM, protected-home replacement, persisted pause and browser profile cookie',flush=True)
        print('CONTAINER', name, 'VOLUME', volume, flush=True)
        review_seconds=int(os.environ.get('TALOS_BROWSER_UI_REVIEW_SECONDS','0'))
        if review_seconds:
            print('UI_REVIEW',url+'/sessions/'+session,flush=True)
            time.sleep(review_seconds)
    finally:
        with (REPORT / 'task-worker.log').open('wb') as worker_log:
            subprocess.run(['docker', 'logs', name], stdout=worker_log, stderr=worker_log)
        subprocess.run(['docker', 'rm', '-f', name], capture_output=True)
        subprocess.run(['docker', 'volume', 'rm', volume], capture_output=True)
        server.send_signal(signal.SIGINT)
        try: server.wait(timeout=15)
        except subprocess.TimeoutExpired: server.kill(); server.wait(timeout=5)
        log.close(); mock.shutdown(); mock.server_close()

if __name__ == '__main__':
    run()
