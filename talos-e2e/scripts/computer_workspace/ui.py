"""Authenticated real Electron/Web product acceptance with isolated homes and TLS pinning."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import time
import yaml
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import ROOT, REPORT, data, free_port, http, require, wait_for
from configuration import ConfigurationCluster
from registration import start_ingress, configure_desktop, public, registered


class ProductCluster(ConfigurationCluster):
    def start_control_plane(self):
        super().start_control_plane()
        models = data(http(self.url + '/v1/models', token=self.users['alice']['access_token']))['models']
        self.model = next(model for model in models if model['id'] == 'deepseek-v4-flash')
        token = self.users['alice']['access_token']
        directory = data(http(self.url + '/v1/fs/directory', {'nodeId': 'embedded', 'parent': str(self.server_home), 'name': 'legacy-project'}, token))['path']
        self.legacy_project = data(http(self.url + '/v1/projects', {'nodeId': 'embedded', 'name': 'Legacy Project', 'directory': directory}, token))['project']
        session, ack = self.start_turn('alice', 'Reply LEGACY_EMBEDDED_OK.', node_id='embedded', project_id=self.legacy_project['project_id'])
        self.events('alice', session, data(ack)['turnId'], 90)
        self.legacy_session = session


class Driver:
    def __init__(self, cluster):
        self.sequence = 0
        self.pending = b''
        self.log = (REPORT / 'ui-driver.log').open('ab')
        self.process = subprocess.Popen(['node', str(ROOT / 'talos-app/desktop/scripts/computer-ui-driver.mjs')],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
        cluster.processes.append(self.process)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)

    def call(self, surface, method, **params):
        self.sequence += 1
        self.process.stdin.write((json.dumps({'id': self.sequence, 'surface': surface, 'method': method, 'params': params}) + '\n').encode())
        self.process.stdin.flush()
        deadline = time.monotonic() + 120
        while time.monotonic() < deadline:
            if b'\n' not in self.pending:
                if not self.selector.select(timeout=0.25):
                    require(self.process.poll() is None, 'UI driver exited')
                    continue
                self.pending += os.read(self.process.stdout.fileno(), 65536)
                continue
            line, self.pending = self.pending.split(b'\n', 1)
            result = json.loads(line)
            require(result['id'] == self.sequence, 'UI driver correlation failed')
            require('result' in result, f'UI {surface}/{method} failed: {result.get("error")}')
            return result['result']
        raise AssertionError(f'UI {surface}/{method} timed out')

    def close(self):
        self.process.stdin.close()
        require(self.process.wait(timeout=30) == 0, 'UI driver did not stop')
        self.selector.close()
        self.log.close()

    def click(self, surface, label, root='main'):
        button = f'Array.from(document.querySelectorAll({json.dumps(root + " button")})).find(b=>b.textContent==={json.dumps(label)}&&!b.disabled)'
        self.call(surface, 'wait', expression=f'Boolean({button})')
        self.call(surface, 'evaluate', expression=f'{button}.click()')

    def wait_final(self, surface, marker):
        self.call(surface, 'wait', expression=f'Array.from(document.querySelectorAll("[data-timeline-response=true]")).some(node=>node.textContent.includes({json.dumps(marker)}))', timeout=120_000)


def passed(case):
    print(f'CASE {case} PASS', flush=True)


def finish_ui_turn(cluster, session, marker, expected_turns=1):
    def settled():
        turns = data(public(cluster, f'/v1/sessions/{session}/history'))['data']
        return turns if len(turns) == expected_turns else None
    turns = wait_for(settled, 'Canonical UI Turn settlement', seconds=120)
    require(all(turn['result']['status'] == 'completed' for turn in turns), 'UI Turn failed or did not settle')
    events = []
    for turn in turns:
        timeline = cluster.events('alice', session, turn['identity']['turnId'], 30)
        require(timeline[-1]['payload']['type'] == 'turnCompleted', 'Missing completed Timeline fact')
        events.extend(timeline)
    text = ''.join(block['content']['data']['text'] for event in events
                   if event['payload']['type'] == 'messageCompleted'
                   for block in event['payload']['data']['blocks']
                   if block['content']['type'] == 'assistantText' and block['content']['data']['phase'] == 'finalAnswer')
    require(marker in text, 'UI execution lacked the actual assistant final answer')


def run(cluster):
    cluster.start()
    start_ingress(cluster)
    cert_public = subprocess.run(['openssl', 'x509', '-in', str(cluster.cert), '-pubkey', '-noout'], capture_output=True, check=True).stdout
    der = subprocess.run(['openssl', 'pkey', '-pubin', '-outform', 'der'], input=cert_public, capture_output=True, check=True).stdout
    import base64
    import hashlib
    spki = base64.b64encode(hashlib.sha256(der).digest()).decode()
    driver = Driver(cluster)
    try:
        home = configure_desktop(cluster, 'electron-a')
        home_d = configure_desktop(cluster, 'electron-d')
        # A committed CreateNode whose response/UUID save was lost: the durable pending barrier survives restart.
        lost = data(public(cluster, '/v1/computers', method='POST', body={'displayName': 'Desktop pending recovery', 'credentialExpiresAt': int(time.time()) + 3600}))['node']['nodeId']
        pending_config = yaml.safe_load((home_d / 'conf/talos.yaml').read_text())
        pending_config['desktop']['enrollment_pending'] = True
        (home_d / 'conf/talos.yaml').write_text(yaml.safe_dump(pending_config, sort_keys=False))
        for name, target_home in [('a', home), ('d', home_d)]:
            driver.call(name, 'launch', name=name, port=free_port(), mainPort=free_port(), home=str(target_home), config=str(target_home / 'conf/talos.yaml'),
                        resourceDir=str(ROOT), report=str(REPORT), spki=spki, executable=os.environ.get('TALOS_E2E_DESKTOP_APP'))
            identity = driver.call(name, 'login', ingress=cluster.ingress, cloud=cluster.cloud_url, token=cluster.users['alice']['access_token'])
            if name == 'd':
                require(identity['nodeId'] is None, 'Pending first enrollment guessed an identity')
                before_total = data(public(cluster, '/v1/computers'))['total']
                driver.call('d', 'evaluate', expression='location.hash="#/computers"')
                driver.call('d', 'wait', expression=f"Boolean(document.querySelector('main [data-computer-id=\"{lost}\"]'))")
                driver.call('d', 'evaluate', expression=f"document.querySelector('main [data-computer-id=\"{lost}\"]').click()")
                driver.click('d', 'Reconnect this computer')
                driver.call('d', 'wait', expression=f'window.talos.workerIdentity().then(r=>r.data.nodeId==={json.dumps(lost)})')
                identity = driver.call('d', 'evaluate', expression='window.talos.workerIdentity().then(r=>r.data)')
                require(data(public(cluster, '/v1/computers'))['total'] == before_total, 'Recovery created a duplicate node')
                driver.call('d', 'evaluate', expression='location.hash="#/"')
                passed('pending_first_registration_recovers_explicit_owned_uuid_through_ui_without_duplicate')
            require(identity['controlPlaneEndpoint'] == cluster.ingress, 'Host source mismatch')
            registered(cluster, identity['nodeId'])
            wait_for(lambda: data(public(cluster, f'/v1/computers/{identity["nodeId"]}'))["available"], "Desktop Connect publication and healthy heartbeat", seconds=60)
            if name == 'a': local_id = identity['nodeId']
            require(driver.call(name, 'evaluate', expression='document.querySelector("[data-computer-id][aria-pressed=true]").getAttribute("data-computer-id")') == identity['nodeId'], 'Desktop did not default to its actual local UUID')
            require((target_home / 'data/talos.db').exists(), 'Desktop management runtime did not create its product DB')
        if os.environ.get('TALOS_E2E_DESKTOP_APP'):
            bundled = str(Path(os.environ['TALOS_E2E_DESKTOP_APP']).resolve().parent.parent / 'Resources/app-server/app-server')
            children = [line.strip() for line in subprocess.check_output(['ps', '-axo', 'pid,command'], text=True).splitlines() if line.endswith(bundled)]
            require(len(children) == 2, 'Installed Desktop did not launch both bundled app-server processes')
            (REPORT / 'bundled-app-server-processes.log').write_text('\n'.join(children) + '\n')
        passed('real_electron_login_ui_polling_default_local_uuid_and_management_db')

        workspace = data(public(cluster, f'/v1/workspace/info?nodeId={local_id}'))
        local_file = Path(workspace['userWorkspaceDir']) / 'electron-local-proof.txt'
        driver.call('a', 'draft', text=f'Use Bash to write ELECTRON_LOCAL_OK to {local_file}. Then reply ELECTRON_LOCAL_OK.')
        local_session = driver.call('a', 'send')
        wait_for(lambda: local_file.exists() and 'ELECTRON_LOCAL_OK' in local_file.read_text(), 'Local Electron execution', seconds=90)
        wait_for(lambda: data(public(cluster, f'/v1/sessions/{local_session}'))['session']['nodeId'] == local_id, 'Canonical Session node')
        finish_ui_turn(cluster, local_session, 'ELECTRON_LOCAL_OK')
        driver.wait_final('a', 'ELECTRON_LOCAL_OK')
        passed('desktop_composer_executes_on_bundled_local_worker_and_streams_results')

        # A known remote is in the first quick page; select it through the visible overflow.
        remote_id = cluster.nodes['alice-a']['node_id']
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'wait', expression=f'Boolean(document.querySelector(\'[role=dialog] [data-computer-id="{remote_id}"]\'))')
        driver.call('a', 'evaluate', expression=f'document.querySelector(\'[role=dialog] [data-computer-id="{remote_id}"]\').click()')
        driver.call('a', 'wait', expression=f'Boolean(document.querySelector(\'aside [data-computer-id="{remote_id}"]\'))')
        workspace = data(public(cluster, f'/v1/workspace/info?nodeId={remote_id}'))
        remote_file = Path(workspace['userWorkspaceDir']) / 'electron-remote-proof.txt'
        driver.call('a', 'draft', text=f'Use Bash to write ELECTRON_REMOTE_OK to {remote_file}. Then reply ELECTRON_REMOTE_OK.')
        remote_session = driver.call('a', 'send')
        wait_for(lambda: remote_file.exists() and 'ELECTRON_REMOTE_OK' in remote_file.read_text(), 'Remote Electron execution', seconds=90)
        require(not list(home.rglob('electron-remote-proof.txt')), 'Remote effect happened on local Desktop')
        require(data(public(cluster, f'/v1/sessions/{remote_session}'))['session']['nodeId'] == remote_id, 'Remote Session affinity lost')
        finish_ui_turn(cluster, remote_session, 'ELECTRON_REMOTE_OK')
        driver.wait_final('a', 'ELECTRON_REMOTE_OK')
        passed('desktop_switches_computer_and_executes_on_remote_worker')

        # A second Desktop recovers the first Desktop's immutable Session and local tag stays its own.
        driver.call('d', 'evaluate', expression=f'location.hash="#/sessions/{local_session}"')
        driver.call('d', 'wait', expression=f'Boolean(document.querySelector(\'[data-computer-id="{local_id}"][aria-pressed=true]\'))')
        driver.wait_final('d', 'ELECTRON_LOCAL_OK')
        label = driver.call('d', 'evaluate', expression=f'document.querySelector(\'[data-computer-id="{local_id}"][aria-pressed=true]\').getAttribute("aria-label")')
        require('This computer' not in label, 'Second Desktop incorrectly labeled another Desktop local')
        passed('second_desktop_reads_first_desktop_history_and_restores_session_computer')
        driver.call('d', 'select', nodeId=local_id)
        second_file = local_file.parent / 'second-desktop-proof.txt'
        driver.call('d', 'draft', text=f'Use Bash to write SECOND_DESKTOP_TO_FIRST_OK to {second_file}. Then reply SECOND_DESKTOP_TO_FIRST_OK.')
        second_session = driver.call('d', 'send')
        wait_for(lambda: second_file.exists() and 'SECOND_DESKTOP_TO_FIRST_OK' in second_file.read_text(), 'Second Desktop executes on first Worker', seconds=90)
        finish_ui_turn(cluster, second_session, 'SECOND_DESKTOP_TO_FIRST_OK')
        driver.wait_final('d', 'SECOND_DESKTOP_TO_FIRST_OK')
        require(data(public(cluster, f'/v1/sessions/{second_session}'))['session']['nodeId'] == local_id and not list(home_d.rglob('second-desktop-proof.txt')), 'Second Desktop used its own Worker')
        passed('second_desktop_dispatches_to_first_desktop_worker_and_receives_real_result')


        driver.call('a', 'select', nodeId=local_id)
        running_file = local_file.parent / 'computer-switch-running-proof.txt'
        command = f'printf RUNNING > {running_file}; sleep 8; printf FINISHED > {running_file}'
        driver.call('a', 'draft', text=f'Run this single Bash command: {command}. Then reply SWITCH_TASK_FINISHED.')
        running_session = driver.call('a', 'send')
        wait_for(lambda: running_file.exists() and running_file.read_text() == 'RUNNING', 'Running task before Computer switch', seconds=90)
        driver.call('a', 'select', nodeId=remote_id)
        require(running_file.read_text() == 'RUNNING', 'Switch did not overlap the running task')
        wait_for(lambda: running_file.read_text() == 'FINISHED', 'Original Worker completed after Computer switch', seconds=90)
        finish_ui_turn(cluster, running_session, 'SWITCH_TASK_FINISHED')
        driver.call('d', 'evaluate', expression=f'location.hash="#/sessions/{running_session}"')
        driver.wait_final('d', 'SWITCH_TASK_FINISHED')
        require(data(public(cluster, f'/v1/sessions/{running_session}'))['session']['nodeId'] == local_id, 'Running Session migrated')
        passed('switching_computer_keeps_running_task_on_original_worker_and_other_client_receives_result')

        # Owner management uses the safe native recovery path, preserving the UUID.
        driver.call('a', 'select', nodeId=local_id)
        driver.call('a', 'evaluate', expression='location.hash="#/computers"')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("input[aria-label=Rename]"))')
        driver.call('a', 'evaluate', expression='(()=>{const i=document.querySelector("input[aria-label=Rename]");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(i,"Renamed desktop");i.dispatchEvent(new Event("input",{bubbles:true}));})()')
        driver.click('a', 'Save')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['displayName'] == 'Renamed desktop', 'Owner rename')
        driver.click('a', 'Disable')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['status'] == 'disabled', 'Owner disable')
        driver.click('a', 'Enable')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['status'] == 'enabled', 'Owner enable')
        config = yaml.safe_load((home / 'conf/talos.yaml').read_text())
        credential_path = Path(config['agent_node']['runner']['credential_file'])
        before_credential = credential_path.read_bytes()
        driver.click('a', 'Reconnect this computer')
        wait_for(lambda: credential_path.read_bytes() != before_credential, 'Native credential recovery')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['available'], 'Recovered native Worker', seconds=60)
        require(driver.call('a', 'evaluate', expression='window.talos.workerIdentity().then(r=>r.data.nodeId)') == local_id, 'Recovery changed UUID')
        passed('ui_owner_rename_disable_enable_and_native_credential_recovery_preserve_uuid')

        # Produce a separate Web build, leaving the packaged Desktop renderer intact.
        web_dir = REPORT / 'web-dist'
        subprocess.run(['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', 'build', '--outDir', str(web_dir), '--emptyOutDir'],
                       env=dict(os.environ, VITE_API_BASE_URL=cluster.ingress, VITE_TALOS_DESKTOP='0'), stdout=(REPORT / 'web-build.log').open('w'), check=True)
        web_port = free_port()
        cluster.start_process('web-static', ['python3', str(Path(__file__).with_name('web_static.py')), str(web_port), str(web_dir)])
        web_url = f'http://127.0.0.1:{web_port}'
        driver.call('web', 'launch', name='web', port=free_port(), home=str(cluster.root / 'chrome-profile'), config='', resourceDir=str(ROOT), report=str(REPORT), spki=spki, webUrl=web_url)
        driver.call('web', 'login', ingress=cluster.ingress, cloud=cluster.cloud_url, token=cluster.users['alice']['access_token'])
        driver.call('web', 'evaluate', expression=f'history.pushState(null,"","/sessions/{local_session}");window.dispatchEvent(new PopStateEvent("popstate"))')
        driver.wait_final('web', 'ELECTRON_LOCAL_OK')
        require(not driver.call('web', 'evaluate', expression='Array.from(document.querySelectorAll("[data-computer-id]")).some(n=>n.getAttribute("aria-label").includes("This computer"))'), 'Web guessed a local Computer')
        passed('authenticated_web_shares_cloud_history_without_guessing_local_identity')
        driver.call('web', 'select', nodeId=local_id)
        web_file = local_file.parent / 'web-to-desktop-proof.txt'
        driver.call('web', 'draft', text=f'Use Bash to write WEB_TO_DESKTOP_OK to {web_file}. Then reply WEB_TO_DESKTOP_OK.')
        web_session = driver.call('web', 'send')
        wait_for(lambda: web_file.exists() and 'WEB_TO_DESKTOP_OK' in web_file.read_text(), 'Web executes on Desktop Worker', seconds=90)
        finish_ui_turn(cluster, web_session, 'WEB_TO_DESKTOP_OK')
        driver.wait_final('web', 'WEB_TO_DESKTOP_OK')
        require(data(public(cluster, f'/v1/sessions/{web_session}'))['session']['nodeId'] == local_id, 'Web task lost Computer affinity')
        passed('web_dispatches_to_desktop_worker_and_receives_real_result')



        driver.call('web', 'evaluate', expression=f'history.pushState(null,"","/sessions/{cluster.legacy_session}");window.dispatchEvent(new PopStateEvent("popstate"))')
        driver.call('web', 'wait', expression="Boolean(document.querySelector('[data-computer-id=embedded][aria-pressed=true]'))")
        driver.wait_final('web', 'LEGACY_EMBEDDED_OK')
        legacy_file = Path(cluster.legacy_project['directory']) / 'legacy-continued.txt'
        driver.call('web', 'draft', text='Use Bash to write LEGACY_CONTINUED_OK to legacy-continued.txt. Then reply LEGACY_CONTINUED_OK.')
        require(driver.call('web', 'send') == cluster.legacy_session, 'Legacy continuation created a new Session')
        wait_for(lambda: legacy_file.exists() and 'LEGACY_CONTINUED_OK' in legacy_file.read_text(), 'Legacy Project continuation', seconds=90)
        finish_ui_turn(cluster, cluster.legacy_session, 'LEGACY_CONTINUED_OK', expected_turns=2)
        driver.wait_final('web', 'LEGACY_CONTINUED_OK')
        driver.call('web', 'evaluate', expression='document.querySelector(`button[aria-label="Collapse sidebar"]`).click()')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("main")?.textContent.includes("Legacy Project"))')
        driver.call('web', 'reload')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("main")?.textContent.includes("Legacy Project"))')
        require(data(public(cluster, f'/v1/sessions/{cluster.legacy_session}'))['session']['nodeId'] == 'embedded', 'Legacy Session migrated')
        passed('legacy_embedded_project_session_continues_and_collapsed_title_survives_reload')
        driver.call('web', 'viewport', width=700, height=820)
        driver.call('web', 'evaluate', expression='document.querySelector(`aside button[aria-label="Open sidebar"]`).focus();document.querySelector(`aside button[aria-label="Open sidebar"]`).click()')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("dialog[open]")) && document.querySelector("dialog").matches(":modal") && document.querySelector("dialog").getBoundingClientRect().width>0')
        driver.call('web', 'screenshot', path=str(REPORT / 'narrow-sidebar.png'))
        driver.call('web', 'key', key='Escape')
        driver.call('web', 'wait', expression='!document.querySelector("dialog[open]") && document.activeElement.getAttribute("aria-label")==="Open sidebar"')
        driver.call('web', 'viewport', width=1280, height=820)
        passed('narrow_sidebar_opens_same_component_and_escape_restores_focus')

        require(public(cluster, '/v1/config/llm')['error']['code'] == 'permissionDenied', 'Legacy history granted shared configuration management')
        driver.call('web', 'evaluate', expression='history.pushState(null,"","/settings/models");window.dispatchEvent(new PopStateEvent("popstate"))')
        driver.call('web', 'wait', expression='location.pathname==="/settings"')
        passed('ordinary_user_recovers_old_embedded_history_without_shared_configuration_permission')

        driver.call('a', 'select', nodeId=remote_id)
        driver.call('a', 'evaluate', expression='location.hash="#/settings/models"')
        driver.call('a', 'reload')
        driver.call('a', 'wait', expression='location.hash==="#/settings/models" && document.querySelector("main")?.textContent.includes("DeepSeek")')
        passed('personal_worker_owner_config_deep_link_survives_metadata_loading_and_reload')
        driver.call('a', 'select', nodeId=local_id)
        driver.call('a', 'draft', text='KEEP_LOCAL_DRAFT')
        driver.call('a', 'select', nodeId=remote_id)
        driver.call('a', 'draft', text='KEEP_REMOTE_DRAFT')
        driver.call('a', 'select', nodeId=local_id)
        require(driver.call('a', 'evaluate', expression='document.querySelector("textarea").value') == 'KEEP_LOCAL_DRAFT', 'Computer draft leaked or was lost')
        driver.call('a', 'reload')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea"))')
        require(driver.call('a', 'evaluate', expression='document.querySelector("textarea").value') == 'KEEP_LOCAL_DRAFT', 'Draft was not restored after reload')
        passed('computer_scoped_drafts_and_recent_selection_survive_reload')

        # Real server pagination, including a visible next page at the UI page size.
        pending = []
        for index in range(22):
            node = data(public(cluster, '/v1/computers', method='POST', body={'displayName': f'page-computer-{index:02}', 'credentialExpiresAt': int(time.time()) + 3600}))
            pending.append(node['node']['nodeId'])
        first = data(public(cluster, '/v1/computers?query=page-computer&limit=2'))
        require(len(first['data']) == 2 and first['nextCursor'] and first['total'] == 22, 'Server pagination was not real')
        second = data(public(cluster, '/v1/computers?query=page-computer&limit=2&cursor=' + first['nextCursor']))
        require(len(second['data']) == 2 and not set(n['nodeId'] for n in first['data']) & set(n['nodeId'] for n in second['data']), 'Cursor repeated nodes')
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("[role=dialog] input"))')
        driver.call('a', 'evaluate', expression='(()=>{const i=document.querySelector("[role=dialog] input");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(i,"page-computer");i.dispatchEvent(new Event("input",{bubbles:true}));})()')
        driver.call('a', 'wait', expression='document.querySelectorAll("[role=dialog] [data-computer-id]").length===20')
        driver.call('a', 'evaluate', expression='Array.from(document.querySelectorAll("[role=dialog] button")).find(b=>b.textContent==="Show more").click()')
        driver.call('a', 'wait', expression='document.querySelectorAll("[role=dialog] [data-computer-id]").length===22')
        driver.call('a', 'evaluate', expression='document.querySelector("[role=dialog] [data-computer-id]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea:disabled"))')
        passed('overflow_server_search_real_pagination_and_offline_no_fallback')

        cluster.stop(cluster.nodes['alice-a']['process'])
        wait_for(lambda: not data(public(cluster, f'/v1/computers/{remote_id}'))['available'], 'Worker disconnected')
        driver.call('a', 'evaluate', expression=f'location.hash="#/sessions/{remote_session}"')
        driver.wait_final('a', 'ELECTRON_REMOTE_OK')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea:disabled"))')
        require(data(public(cluster, f'/v1/sessions/{remote_session}'))['session']['nodeId'] == remote_id, 'Offline history retargeted')
        passed('offline_pinned_session_preserves_cloud_history_and_disables_execution')

        driver.call('a', 'select', nodeId=local_id)
        for name, theme in [('a', 'light'), ('d', 'dark')]:
            driver.call(name, 'evaluate', expression=f'document.documentElement.dataset.theme="{theme}"')
            driver.call(name, 'screenshot', path=str(REPORT / f'electron-{theme}.png'))
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'screenshot', path=str(REPORT / 'computer-overflow.png'))
        driver.call('a', 'evaluate', expression='document.querySelector("[role=dialog] input").dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}))')
        require(driver.call('a', 'evaluate', expression='!document.querySelector("[role=dialog]") && document.activeElement.getAttribute("aria-haspopup")==="dialog"'), 'Escape did not restore focus')
        driver.call('web', 'screenshot', path=str(REPORT / 'web-computer-session.png'))
        passed('real_themes_overflow_keyboard_focus_and_product_screenshots')

        driver.call('a', 'evaluate', expression='location.hash="#/settings/general"')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("main select option[value=zh-CN]"))')
        driver.call('a', 'evaluate', expression='(()=>{const select=document.querySelector("main select");select.value="zh-CN";select.dispatchEvent(new Event("change",{bubbles:true}));})()')
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("[role=dialog] input"))')
        driver.call('a', 'evaluate', expression='(()=>{const input=document.querySelector("[role=dialog] input");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(input,"Renamed desktop");input.dispatchEvent(new Event("input",{bubbles:true}));})()')
        driver.call('a', 'wait', expression='document.querySelector("[role=dialog]")?.textContent.includes("本机")')
        driver.call('a', 'screenshot', path=str(REPORT / 'computer-chinese.png'))
        passed('computer_menu_and_local_label_follow_real_language_setting')

        driver.call('web', 'evaluate', expression='document.querySelector("aside button[aria-expanded]:not([aria-haspopup])").click()')
        driver.click('web', 'Log out', root='aside')
        driver.call('web', 'wait', expression='!document.querySelector("[data-computer-id]") && location.pathname.startsWith("/auth/") && document.body.textContent.includes("Signed out of Talos")')
        driver.call('web', 'login', ingress=cluster.ingress, cloud=cluster.cloud_url, token=cluster.users['bob']['access_token'])
        bob_id = cluster.nodes['bob-a']['node_id']
        require(not driver.call('web', 'evaluate', expression='Boolean(document.querySelector("[data-computer-id][aria-pressed=true]"))'), 'New Web account reused a Computer selection')
        driver.call('web', 'select', nodeId=bob_id)
        driver.call('web', 'wait', expression=f'Boolean(document.querySelector(\'[data-computer-id="{bob_id}"][aria-pressed=true]\'))')
        require(not driver.call('web', 'evaluate', expression='document.body.textContent.includes("ELECTRON_LOCAL_OK") || document.body.textContent.includes("page-computer")'), 'Old account view survived logout')
        driver.call('web', 'evaluate', expression=f'history.pushState(null,"","/sessions/{local_session}");window.dispatchEvent(new PopStateEvent("popstate"))')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("main [role=alert]"))')
        require(not driver.call('web', 'evaluate', expression='document.body.textContent.includes("ELECTRON_LOCAL_OK")'), 'Previous account Session cache disclosed history')
        passed('real_logout_and_second_account_hide_previous_nodes_and_session_cache')
    finally:
        driver.close()


if __name__ == '__main__':
    cluster = ProductCluster(worker_transport='outbound')
    try:
        run(cluster)
    finally:
        cluster.close()
