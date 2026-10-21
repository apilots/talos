"""Real central Computer product UI; multi-control-plane data sharing belongs to group 19."""
import json
import os
from pathlib import Path
import selectors
import shutil
import subprocess
import sys
import time
import yaml
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import ROOT, REPORT, data, free_port, http, require, wait_for
from environment import EnvironmentCluster
from registration import start_ingress, public


class Driver:
    def __init__(self, cluster):
        self.sequence = 0
        self.pending = b''
        self.log = (REPORT / 'ui-driver.log').open('ab')
        self.process = subprocess.Popen(['node', str(ROOT / 'talos-app/desktop/scripts/computer-ui-driver.mjs')],
                                        executable=shutil.which('node'), close_fds=False,
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


def finish_ui_turn(cluster, session, marker, expected_turns=1, user='alice'):
    def settled():
        response = public(cluster, f'/v1/sessions/{session}/history', user=user)
        if response.get('error', {}).get('code') == 'resourceNotFound':
            return None  # The composer navigates before the first start acknowledgement.
        turns = data(response)['data']
        return turns if len(turns) == expected_turns else None
    turns = wait_for(settled, 'Canonical UI Turn settlement', seconds=120)
    require(all(turn['result']['status'] == 'completed' for turn in turns), 'UI Turn failed or did not settle')
    events = []
    for turn in turns:
        timeline = cluster.events(user, session, turn['identity']['turnId'], 30)
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
    web_dir = REPORT / 'web-dist'
    with (REPORT / 'web-build.log').open('w') as log:
        subprocess.run(['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', 'build', '--outDir', str(web_dir), '--emptyOutDir'],
                       env=dict(os.environ, VITE_API_BASE_URL=cluster.ingress, VITE_TALOS_DESKTOP='0'), stdout=log, check=True)
    web_port = free_port()
    cluster.start_process('web-static', ['python3', str(Path(__file__).with_name('web_static.py')), str(web_port), str(web_dir)])
    driver = Driver(cluster)
    primary, remote = [cluster.nodes[name] for name in ('alice-a', 'alice-b')]
    local_id, remote_id = primary['node_id'], remote['node_id']
    def navigate(surface, path):
        driver.call(surface, 'evaluate', expression=f'history.pushState(null,"",{json.dumps(path)});window.dispatchEvent(new PopStateEvent("popstate"))')
    try:
        for name in ('a', 'web'):
            driver.call(name, 'launch', name=name, port=free_port(), home=str(cluster.root / f'chrome-{name}'), config='',
                        resourceDir=str(ROOT), report=str(REPORT), spki=spki, webUrl=f'http://127.0.0.1:{web_port}', headless=True)
            driver.call(name, 'login', ingress=cluster.ingress, cloud=cluster.cloud_url, token=cluster.users['alice']['access_token'])
            driver.call(name, 'select', nodeId=local_id)
        passed('real_web_device_login_selects_owned_outbound_computers')

        driver.call('a', 'reload')
        driver.call('a', 'wait', expression='Boolean(document.querySelector(`button[aria-label="Collapse sidebar"]`))')
        require(driver.call('a', 'evaluate', expression='document.activeElement!==document.querySelector(`button[aria-label="Collapse sidebar"]`)'), 'Refresh automatically focused Sidebar Collapse')
        driver.call('a', 'screenshot', path=str(REPORT / 'sidebar-default.png'))
        driver.call('a', 'evaluate', expression='document.querySelector(`button[aria-label="Collapse sidebar"]`).focus()')
        # Explicit focus is allowed; the shared resource journey separately checks
        # real keyboard focus-visible styling. Refresh must never assign it.
        require(driver.call('a', 'evaluate', expression='document.activeElement===document.querySelector(`button[aria-label="Collapse sidebar"]`)'), 'Explicit focus did not reach Sidebar Collapse')
        driver.call('a', 'screenshot', path=str(REPORT / 'sidebar-focus.png'))
        driver.call('a', 'evaluate', expression='document.activeElement.blur()')
        require(not driver.call('a', 'evaluate', expression='Array.from(document.querySelectorAll("[data-computer-id]")).some(n=>n.getAttribute("aria-label").includes("This computer"))'), 'Web guessed a local Computer')

        workspace = data(public(cluster, f'/v1/workspace/info?nodeId={local_id}'))
        local_file = Path(workspace['userWorkspaceDir']) / 'web-worker-proof.txt'
        driver.call('a', 'draft', text='Use Bash to write WEB_WORKER_OK to web-worker-proof.txt. Then reply WEB_WORKER_OK.')
        local_session = driver.call('a', 'send')
        wait_for(lambda: local_file.exists() and 'WEB_WORKER_OK' in local_file.read_text(), 'Actual selected Worker side effect', seconds=90)
        finish_ui_turn(cluster, local_session, 'WEB_WORKER_OK')
        driver.wait_final('a', 'WEB_WORKER_OK')
        require(data(public(cluster, f'/v1/sessions/{local_session}'))['session']['nodeId'] == local_id, 'Session lost Computer affinity')
        passed('web_composer_executes_selected_worker_and_streams_real_result')

        navigate('web', f'/sessions/{local_session}')
        driver.wait_final('web', 'WEB_WORKER_OK')
        driver.call('web', 'draft', text='Use Bash to read web-worker-proof.txt, then append WEB_CONTINUED_OK. Reply WEB_CONTINUED_OK.')
        require(driver.call('web', 'send') == local_session, 'Continuation created another Session')
        finish_ui_turn(cluster, local_session, 'WEB_CONTINUED_OK', expected_turns=2)
        driver.wait_final('web', 'WEB_CONTINUED_OK')
        require('WEB_CONTINUED_OK' in local_file.read_text(), 'Continuation ran on another Worker')
        passed('two_clients_continue_same_computer_session_through_one_control_plane')

        driver.call('a', 'select', nodeId=remote_id)
        remote_workspace = data(public(cluster, f'/v1/workspace/info?nodeId={remote_id}'))
        remote_file = Path(remote_workspace['userWorkspaceDir']) / 'second-worker-proof.txt'
        driver.call('a', 'draft', text='Use Bash to write SECOND_WORKER_OK to second-worker-proof.txt. Reply SECOND_WORKER_OK.')
        remote_session = driver.call('a', 'send')
        finish_ui_turn(cluster, remote_session, 'SECOND_WORKER_OK')
        driver.wait_final('a', 'SECOND_WORKER_OK')
        require(remote_file.read_text().strip() == 'SECOND_WORKER_OK', 'Computer switch used another filesystem')
        passed('computer_switch_preserves_worker_filesystem_and_session_affinity')

        driver.call('a', 'select', nodeId=local_id)
        running_file = local_file.parent / 'running-switch.txt'
        driver.call('a', 'draft', text='Use Bash once: printf RUNNING > running-switch.txt; sleep 15; printf FINISHED > running-switch.txt. Reply SWITCH_TASK_FINISHED.')
        running_session = driver.call('a', 'send')
        wait_for(lambda: running_file.exists() and running_file.read_text() == 'RUNNING', 'Actual task started before switch', seconds=90)
        driver.call('a', 'select', nodeId=remote_id)
        wait_for(lambda: running_file.read_text() == 'FINISHED', 'Task finishes on original Computer after UI switch', seconds=90)
        finish_ui_turn(cluster, running_session, 'SWITCH_TASK_FINISHED')
        navigate('web', f'/sessions/{running_session}')
        driver.wait_final('web', 'SWITCH_TASK_FINISHED')
        require(data(public(cluster, f'/v1/sessions/{running_session}'))['session']['nodeId'] == local_id, 'Running Session migrated')
        passed('switching_computer_keeps_running_task_and_realtime_result_on_original_worker')


        driver.call('a', 'select', nodeId=local_id)
        navigate('a', '/computers')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("input[aria-label=Rename]"))')
        driver.call('a', 'evaluate', expression='(()=>{const i=document.querySelector("input[aria-label=Rename]");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(i,"Renamed computer");i.dispatchEvent(new Event("input",{bubbles:true}));})()')
        driver.click('a', 'Save')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['displayName'] == 'Renamed computer', 'Owner rename')
        driver.click('a', 'Disable')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['status'] == 'disabled', 'Owner disable')
        cluster.stop(primary['process'])
        driver.click('a', 'Enable')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['status'] == 'enabled', 'Owner enable')
        cluster.start_worker('alice-a')
        wait_for(lambda: data(public(cluster, f'/v1/computers/{local_id}'))['available'], 'Owned Worker rejoins')
        passed('owner_rename_disable_enable_preserve_computer_uuid')

        project_directory = data(public(cluster, '/v1/fs/directory', method='POST', body={
            'nodeId': local_id, 'parent': str(local_file.parent), 'name': 'ui-project'}))['path']
        project = data(public(cluster, '/v1/projects', method='POST', body={
            'nodeId': local_id, 'name': 'Computer Project', 'directory': project_directory}))['project']
        project_session, _, _ = cluster.turn('alice', 'Reply PROJECT_READY.', node_id=local_id, project_id=project['project_id'])
        navigate('web', f'/sessions/{project_session}')
        driver.wait_final('web', 'PROJECT_READY')
        driver.call('web', 'draft', text='Use Bash to write PROJECT_CONTINUED_OK to continued.txt. Reply PROJECT_CONTINUED_OK.')
        require(driver.call('web', 'send') == project_session, 'Project continuation created a new Session')
        finish_ui_turn(cluster, project_session, 'PROJECT_CONTINUED_OK', expected_turns=2)
        driver.wait_final('web', 'PROJECT_CONTINUED_OK')
        require((Path(project_directory) / 'continued.txt').read_text().strip() == 'PROJECT_CONTINUED_OK', 'Project continuation used another directory')
        driver.call('web', 'evaluate', expression='document.querySelector(`button[aria-label="Collapse sidebar"]`).click()')
        project_title = data(public(cluster, f'/v1/sessions/{project_session}'))['session']['title']
        require(bool(project_title), 'Project Session has no task title')
        project_header = ('location.pathname === ' + json.dumps(f'/sessions/{project_session}')
            + ' && Array.from(document.querySelectorAll("header")).some(header => '
            + '!header.closest("[inert], [aria-hidden=true]") && header.textContent.includes("Computer Project") '
            + '&& header.textContent.includes(' + json.dumps(project_title) + '))')
        driver.call('web', 'wait', expression=project_header)
        driver.call('web', 'screenshot', path=str(REPORT / 'project-header-collapsed.png'))
        driver.call('web', 'reload')
        driver.call('web', 'wait', expression=project_header)
        driver.call('web', 'screenshot', path=str(REPORT / 'project-header-reloaded.png'))
        passed('owned_project_continuation_and_collapsed_title_survive_reload')
        driver.call('web', 'viewport', width=700, height=820)
        driver.call('web', 'evaluate', expression='document.querySelector(`header button[aria-label="Open sidebar"]`).focus();document.querySelector(`header button[aria-label="Open sidebar"]`).click()')
        driver.call('web', 'wait', expression='Boolean(document.querySelector("dialog[open]")) && document.querySelector("dialog").matches(":modal")')
        driver.call('web', 'screenshot', path=str(REPORT / 'narrow-sidebar.png'))
        driver.call('web', 'key', key='Escape')
        driver.call('web', 'wait', expression='!document.querySelector("dialog[open]") && document.activeElement.getAttribute("aria-label")==="Open sidebar"')
        driver.call('web', 'viewport', width=1280, height=820)
        passed('narrow_sidebar_escape_restores_focus')

        navigate('a', '/settings/models')
        driver.call('a', 'reload')
        driver.call('a', 'wait', expression='location.pathname==="/settings/models" && document.querySelector("main")?.textContent.includes("DeepSeek")')
        passed('owner_model_configuration_deep_link_survives_loading_and_reload')
        driver.call('a', 'select', nodeId=local_id)
        driver.call('a', 'draft', text='KEEP_FIRST_DRAFT')
        driver.call('a', 'select', nodeId=remote_id)
        driver.call('a', 'draft', text='KEEP_SECOND_DRAFT')
        driver.call('a', 'select', nodeId=local_id)
        driver.call('a', 'reload')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea"))')
        require(driver.call('a', 'evaluate', expression='document.querySelector("textarea").value') == 'KEEP_FIRST_DRAFT', 'Computer draft lost after switching or reload')
        passed('computer_scoped_drafts_survive_reload')

        for index in range(22):
            data(public(cluster, '/v1/computers', method='POST', body={'displayName': f'page-computer-{index:02}', 'credentialExpiresAt': int(time.time()) + 3600}))
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("[role=dialog] input"))')
        driver.call('a', 'evaluate', expression='(()=>{const i=document.querySelector("[role=dialog] input");Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,"value").set.call(i,"page-computer");i.dispatchEvent(new Event("input",{bubbles:true}));})()')
        driver.call('a', 'wait', expression='document.querySelectorAll("[role=dialog] [data-computer-id]").length===20')
        driver.call('a', 'evaluate', expression='Array.from(document.querySelectorAll("[role=dialog] button")).find(b=>b.textContent==="Show more").click()')
        driver.call('a', 'wait', expression='document.querySelectorAll("[role=dialog] [data-computer-id]").length===22')
        driver.call('a', 'evaluate', expression='document.querySelector("[role=dialog] [data-computer-id]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea:disabled"))')
        passed('overflow_search_pagination_and_offline_computer_view')

        cluster.stop(remote['process'])
        wait_for(lambda: not data(public(cluster, f'/v1/computers/{remote_id}'))['available'], 'Worker disconnected')
        navigate('a', f'/sessions/{remote_session}')
        driver.wait_final('a', 'SECOND_WORKER_OK')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("textarea:disabled"))')
        require(data(public(cluster, f'/v1/sessions/{remote_session}'))['session']['nodeId'] == remote_id, 'Offline history retargeted')
        passed('offline_session_preserves_history_and_computer_binding')
        driver.call('a', 'select', nodeId=local_id)
        for name, theme in [('a', 'light'), ('web', 'dark')]:
            driver.call(name, 'evaluate', expression=f'document.documentElement.dataset.theme="{theme}"')
            driver.call(name, 'screenshot', path=str(REPORT / f'computer-{theme}.png'))
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'screenshot', path=str(REPORT / 'computer-overflow.png'))
        driver.call('a', 'evaluate', expression='document.querySelector("[role=dialog] input").dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}))')
        require(driver.call('a', 'evaluate', expression='!document.querySelector("[role=dialog]") && document.activeElement.getAttribute("aria-haspopup")==="dialog"'), 'Escape did not restore focus')
        passed('real_light_dark_themes_and_overflow_keyboard_focus')

        navigate('a', '/settings/general')
        driver.call('a', 'wait', expression='Boolean(document.querySelector("main select option[value=zh-CN]"))')
        driver.call('a', 'evaluate', expression='(()=>{const select=document.querySelector("main select");select.value="zh-CN";select.dispatchEvent(new Event("change",{bubbles:true}));})()')
        driver.call('a', 'evaluate', expression='document.querySelector("button[aria-haspopup=dialog]").click()')
        driver.call('a', 'wait', expression='Boolean(document.querySelector(`[role=dialog] input[placeholder="搜索 Computer"]`))')
        driver.call('a', 'screenshot', path=str(REPORT / 'computer-chinese.png'))
        driver.call('a', 'evaluate', expression='document.querySelector("[role=dialog] input").dispatchEvent(new KeyboardEvent("keydown",{key:"Escape",bubbles:true}))')
        driver.call('a', 'evaluate', expression='(()=>{const select=document.querySelector("main select");select.value="en";select.dispatchEvent(new Event("change",{bubbles:true}));})()')
        passed('computer_menu_follows_product_language_setting')

        driver.call('a', 'evaluate', expression='document.querySelector("aside button[aria-expanded]:not([aria-haspopup])").click()')
        driver.click('a', 'Log out', root='aside')
        driver.call('a', 'wait', expression='location.pathname.startsWith("/auth/") && document.body.textContent.includes("Signed out of Talos")')
        driver.call('a', 'login', ingress=cluster.ingress, cloud=cluster.cloud_url, token=cluster.users['bob']['access_token'])
        bob = cluster.nodes['bob-a']
        driver.call('a', 'select', nodeId=bob['node_id'])
        driver.call('a', 'draft', text="Use Bash once: printf 'BOB_WORKER_OK' > bob-ui-proof.txt. Then reply BOB_WORKER_OK.")
        bob_session = driver.call('a', 'send')
        finish_ui_turn(cluster, bob_session, 'BOB_WORKER_OK', user='bob')
        cluster.verify_file('bob-a', 'bob-ui-proof.txt', 'BOB_WORKER_OK')
        driver.wait_final('a', 'BOB_WORKER_OK')
        passed('logout_and_second_account_execute_only_its_owned_computer')
    finally:
        driver.close()


if __name__ == '__main__':
    cluster = EnvironmentCluster(worker_transport='outbound')
    try:
        run(cluster)
    finally:
        cluster.close()
