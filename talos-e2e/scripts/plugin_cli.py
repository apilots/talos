"""Exercise the CLI against a catalog prepared by the original Plugin API."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import urllib.request

ROOT = Path(__file__).resolve().parents[2]

def git(repo, *args):
    return subprocess.check_output(['git', '-C', str(repo), *args], text=True).strip()


def prepare(url):
    WORKER_HOME = Path(os.environ['TALOS_HOME'])
    REPORT = Path(os.environ['TALOS_SCENARIO_REPORT_DIR'])
    REPO = WORKER_HOME / 'plugin-cli-fixture'
    STATE = REPORT / 'plugin-cli-catalog.json'
    skill = REPO / 'skills/cli-skill'
    mcp = REPO / 'mcp/cli-mcp'
    skill.mkdir(parents=True)
    mcp.mkdir(parents=True)
    (skill / 'SKILL.md').write_text('---\nname: cli-skill\ndescription: CLI lifecycle fixture.\n---\nVERSION_ONE\n')
    shutil.copy2(ROOT / 'talos-rs/crates/action/mcp/tests/fixtures/server.py', mcp / 'server.py')
    (mcp / 'talos-mcp.json').write_text(json.dumps({
        'name': 'cli-mcp', 'transport': {'type': 'stdio', 'command': sys.executable,
        'args': ['server.py', str(WORKER_HOME / 'cli-peer'), 'normal'], 'env': {}, 'cwd': None}}))
    (mcp / 'README.md').write_text('VERSION_ONE\n')
    git(REPO, 'init', '-b', 'main')
    git(REPO, 'config', 'user.name', 'Talos E2E')
    git(REPO, 'config', 'user.email', 'e2e@example.test')
    git(REPO, 'add', '.')
    git(REPO, 'commit', '-m', 'fixture: first version')
    request = urllib.request.Request(url + '/v1/plugin/sources', data=json.dumps({
        'source': {'kind': 'git', 'url': REPO.as_uri(), 'reference': 'main', 'sparse_paths': []},
        'refresh': True}).encode(), headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=60) as response:
        catalog = json.load(response)['data']
    assert {plugin['kind'] for plugin in catalog['plugins']} == {'skill', 'mcp_server'}
    STATE.write_text(json.dumps({'plugins': catalog['plugins'], 'commit': git(REPO, 'rev-parse', 'HEAD')}))


def run():
    WORKER_HOME = Path(os.environ['TALOS_HOME'])
    REPORT = Path(os.environ['TALOS_SCENARIO_REPORT_DIR'])
    REPO = WORKER_HOME / 'plugin-cli-fixture'
    STATE = REPORT / 'plugin-cli-catalog.json'
    catalog = json.loads(STATE.read_text())
    evidence = []
    def cli(*args):
        return invoke_cli(args, evidence)
    installed = []
    for plugin in catalog['plugins']:
        result = cli('install', plugin['id'], '--commit', catalog['commit'], '--disabled', '--node', 'embedded')
        install = result['install']
        assert not install['enabled'] and Path(install['installed_path']).is_relative_to(WORKER_HOME)
        assert install['source']['commit'] == catalog['commit']
        installed.append(install)
        assert cli('enable', install['id'])['plugin']['status']['enabled']
        assert not cli('disable', install['id'], '--node', 'embedded')['plugin']['status']['enabled']
    listed = cli('list', '--node', 'embedded')['plugins']
    assert all(any(row['id'] == install['plugin_id'] for row in listed) for install in installed)
    for path in [REPO / 'skills/cli-skill/SKILL.md', REPO / 'mcp/cli-mcp/README.md']:
        path.write_text(path.read_text().replace('VERSION_ONE', 'VERSION_TWO'))
    git(REPO, 'add', '.')
    git(REPO, 'commit', '-m', 'fixture: second version')
    commit = git(REPO, 'rev-parse', 'HEAD')
    for install in installed:
        updated = cli('update', install['id'], '--commit', commit, '--node', 'embedded')['install']
        assert updated['id'] == install['id'] and updated['source']['commit'] == commit and not updated['enabled']
        contents = 'SKILL.md' if install['kind'] == 'skill' else 'README.md'
        assert 'VERSION_TWO' in (Path(updated['installed_path']) / contents).read_text()
        assert cli('uninstall', install['id'])['uninstalled']
        assert not Path(install['installed_path']).exists()
    remaining = {row['id'] for row in cli('list')['plugins']}
    assert not any(install['plugin_id'] in remaining for install in installed)
    (REPORT / 'plugin-cli-acceptance.json').write_text(json.dumps({
        'status': 'passed', 'kinds': ['skill', 'mcp_server'], 'commit': commit,
        'checks': ['install', 'list', 'enable', 'disable', 'update', 'uninstall', 'explicit embedded target', 'disabled update preserved'],
        'commands': evidence}, indent=2))


def invoke_cli(args, evidence, environment=None):
    environment = os.environ if environment is None else environment
    result = subprocess.run([environment['TALOS_E2E_CLI_BIN'], 'plugin', *args],
        env=environment, text=True, capture_output=True, timeout=90)
    assert result.returncode == 0, result.stderr
    payload = json.loads(result.stdout)
    evidence.append({'args': list(args), 'result': payload})
    return payload


def remote_cli(url, node, container, environment, http, data, report):
    catalog_root = '/home/talos/.talos/remote-cli-catalog'
    files = {
        'skills/remote-cli-skill/SKILL.md': '---\nname: remote-cli-skill\ndescription: Remote CLI evidence.\n---\nVERSION_ONE\n',
        'mcp/remote_cli_mcp/talos-mcp.json': json.dumps({'name': 'remote_cli_mcp', 'transport': {
            'type': 'stdio', 'command': '/usr/bin/python3',
            'args': ['server.py', '/home/talos/.talos/remote-cli-peer', 'normal'], 'env': {}, 'cwd': None}}),
        'mcp/remote_cli_mcp/server.py': (ROOT / 'talos-rs/crates/action/mcp/tests/fixtures/server.py').read_text(),
        'mcp/remote_cli_mcp/README.md': 'VERSION_ONE\n',
    }
    def write_files():
        script = "import json,sys;from pathlib import Path;root=Path(sys.argv[1]);files=json.load(sys.stdin);[( (root/name).parent.mkdir(parents=True,exist_ok=True),(root/name).write_text(text)) for name,text in files.items()]"
        subprocess.run(['docker', 'exec', '-i', container, 'python3', '-c', script, catalog_root],
            input=json.dumps(files), text=True, check=True, capture_output=True)
    write_files()
    catalog = data(http(url + '/v1/plugin/sources', {'nodeId': node, 'source': {
        'kind': 'local_folder', 'path': catalog_root, 'sparse_paths': []}, 'refresh': True}))
    assert {plugin['kind'] for plugin in catalog['plugins']} == {'skill', 'mcp_server'}
    evidence = []
    def cli(*args):
        return invoke_cli([*args, '--node', node], evidence, environment)
    installed = []
    for plugin in catalog['plugins']:
        install = cli('install', plugin['id'], '--disabled')['install']
        assert install['installed_path'].startswith('/home/talos/.talos/plugins/') and not install['enabled']
        installed.append(install)
        assert cli('enable', install['id'])['plugin']['status']['enabled']
        assert not cli('disable', install['id'])['plugin']['status']['enabled']
    listed = {row['id'] for row in cli('list')['plugins']}
    assert all(install['plugin_id'] in listed for install in installed)
    files = {name: text.replace('VERSION_ONE', 'VERSION_TWO') for name, text in files.items()}
    write_files()
    for install in installed:
        updated = cli('update', install['id'])['install']
        assert updated['id'] == install['id'] and not updated['enabled']
        assert updated['content_digest'] != install['content_digest']
        assert cli('uninstall', install['id'])['uninstalled']
        assert subprocess.run(['docker', 'exec', container, 'test', '-e', install['installed_path']]).returncode != 0
    local = json.loads((Path(environment['TALOS_HOME']) / 'plugins/plugin-installs.json').read_text())['installs']
    assert not any(row['name'] in ('remote_cli_mcp', 'remote-cli-skill') for row in local)
    (report / 'remote-cli-acceptance.json').write_text(json.dumps({
        'status': 'passed', 'nodeId': node, 'kinds': ['skill', 'mcp_server'],
        'clientHasNoRemoteInstall': True, 'commands': evidence}, indent=2))
    print('PASS real CLI -> Service -> outbound Docker Worker Plugin lifecycle; no remote installation on client', flush=True)


if __name__ == '__main__':
    if sys.argv[1] == 'prepare':
        prepare(sys.argv[2])
    else:
        run()
