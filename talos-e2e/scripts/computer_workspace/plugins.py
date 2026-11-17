"""Real plugin/MCP lifecycle through authenticated outbound Workers."""
from pathlib import Path
import json
from shlex import quote
import sys
import uuid
from urllib.parse import urlencode
from fixture import data, http, require, wait_for
from scenarios import final_answer, passed


def plugins(cluster):
    token = cluster.users['alice']['access_token']
    a, b = [cluster.nodes[name] for name in ('alice-a', 'alice-b')]
    def call(path, body, node=a, method='POST'):
        return data(http(cluster.url + path, {**body, 'nodeId': node['node_id']}, token, method=method))
    def query(path, node=a):
        return data(http(cluster.url + path + '?' + urlencode({'nodeId': node['node_id']}), token=token))
    bob_query = urlencode({'nodeId': cluster.nodes['bob-a']['node_id']})
    bob_sources = data(http(cluster.url+'/v1/plugin/sources?'+bob_query, token=cluster.users['bob']['access_token']))
    require(isinstance(bob_sources['sources'], list), 'Bob could not access his Computer plugin catalog')
    passed('each_owner_manages_personal_worker_plugins')

    repo = a['home']/'marketplace'
    skill = repo/'skills'/'computer-plugin'
    skill.mkdir(parents=True)
    skill_file = skill/'SKILL.md'
    skill_file.write_text('---\nname: computer-plugin\ndescription: Target plugin evidence.\n---\n\nPLUGIN_VERSION_ONE\n')
    added=call('/v1/plugin/sources', {'source': {'kind':'local_folder','path':str(repo),'sparse_paths':['skills']},'refresh':True})
    source_id=added['source']['id']
    plugin=added['plugins'][0]
    require(any(s['id']==source_id for s in query('/v1/plugin/sources')['sources']), 'Source not persisted on target')
    require(not any(s['id']==source_id for s in query('/v1/plugin/sources', b)['sources']), 'Source copied to another node')
    search=call('/v1/plugin/search', {'query':'computer-plugin','source_id':source_id,'kind':'skill','status':None,'refresh':'cache_only','limit':10})
    require(search['plugins'][0]['id']==plugin['id'], 'Search changed canonical identity')
    read=call('/v1/plugin/read', {'plugin_id':plugin['id']})
    require(any('PLUGIN_VERSION_ONE' in doc['contents'] for doc in read['documents']), 'Read used another filesystem')
    installed=call('/v1/plugin/install', {'target':{'kind':'plugin','plugin_id':plugin['id'],'expected_source_commit':None},'enable':True,'conflict_policy':'reject'})
    install=installed['install']; iid=install['id']
    require(Path(install['installed_path']).is_relative_to(a['home']), 'Install escaped selected Worker')
    require(Path(install['installed_path']).exists(), 'Install did not produce Worker files')
    passed('source_search_read_and_install_execute_on_selected_worker')
    skill_file.write_text(skill_file.read_text().replace('VERSION_ONE','VERSION_TWO'))
    refreshed=call('/v1/plugin/sources/refresh', {'source_id':source_id})
    require(refreshed['plugins'], 'Refresh lost source catalog')
    updated=call('/v1/plugin/update', {'install_id':iid,'expected_source_commit':None,'conflict_policy':'reject_local_changes'})
    require(updated['install']['content_digest'] != install['content_digest'], 'Update did not change artifact')
    call('/v1/plugin/disable', {'install_id':iid})
    listing=call('/v1/plugin/list', {'kind':None,'status':'disabled','source_id':source_id,'include_disabled':True})
    require(listing['plugins'], 'Disable did not affect target status')
    call('/v1/plugin/enable', {'install_id':iid})
    passed('source_refresh_update_disable_and_enable_preserve_domain_behavior')

    secret='worker-secret-'+uuid.uuid4().hex
    evidence=a['home']/'mcp-tool-evidence'
    definition={'name':'computer_evidence','display_name':None,'description':None,'transport':{'type':'stdio','command':sys.executable,'args':[str(Path(__file__).with_name('mcp_stdio.py'))],'env':{'EVIDENCE_FILE':str(evidence)},'cwd':str(a['home'])}}
    mcp=call('/v1/plugin/install', {'target':{'kind':'manual_mcp_server','server':definition,'credentials':{'authentication':{'type':'none'},'env':{'PRIVATE_KEY':secret}}},'enable':True,'conflict_policy':'reject'})
    owner={'type':'plugin','installId':mcp['install']['id']}
    require(secret not in json.dumps(mcp), 'Install response exposed credential')
    require(any(secret in p.read_text() for p in (a['home']/'mcp-credentials').glob('*.json')), 'Credential not saved on Worker')
    require(not any(secret in p.read_text() for p in (cluster.server_home/'mcp-credentials').glob('*.json')), 'Credential copied to control plane')
    read=call('/v1/mcp/servers/auth/read', {'server':owner})
    require(read['state']['type']=='credentialsStored', 'Authentication status changed')
    session, ack=cluster.start_turn('alice', 'Call ToolSearch with exact query mcp__computer_evidence__record_evidence, then call that exact MCP tool once. Reply COMPUTER_MCP_EXECUTED.', node_id=a['node_id'])
    events=cluster.events('alice', session, data(ack)['turnId'], 120)
    require(events[-1]['payload']['type']=='turnCompleted', 'Actual MCP execution failed')
    require(evidence.read_text()==secret, 'Real MCP process did not receive private Worker credential')
    replacement='replacement-'+uuid.uuid4().hex
    call('/v1/mcp/servers/credentials', {'server':owner,'change':{'type':'replace','credentials':{'authentication':{'type':'none'},'env':{'PRIVATE_KEY':replacement}}}}, method='PUT')
    session, ack=cluster.start_turn('alice', 'Call ToolSearch for mcp__computer_evidence__record_evidence, then call that exact tool once. Reply COMPUTER_MCP_EXECUTED.', node_id=a['node_id'])
    require(cluster.events('alice',session,data(ack)['turnId'],120)[-1]['payload']['type']=='turnCompleted', 'Credential replacement broke actual tool')
    require(evidence.read_text()==replacement,'MCP process retained old private credential')
    require(secret not in json.dumps(query('/v1/mcp/servers')), 'Status exposed credential')
    passed('write_only_credentials_and_real_mcp_tool_use_worker_owned_files')

    started = a['home'] / ('active-plugin-' + uuid.uuid4().hex)
    release = started.with_suffix('.release')
    completed = started.with_suffix('.completed')
    command = (
        f'touch {quote(str(started))} && '
        f'for attempt in {{1..45}}; do if [ -f {quote(str(release))} ]; then '
        f'touch {quote(str(completed))} && echo PLUGIN_GATE_OK; exit $?; fi; sleep 1; done; exit 1'
    )
    session, ack = cluster.start_turn(
        'alice', f'Use Bash once to run this exact command: `{command}` '
        'Then reply with the command output.', node_id=a['node_id'])
    turn_id = data(ack)['turnId']
    try:
        wait_for(started.exists, 'Worker Bash started before plugin publication', seconds=60)
        disabled = call('/v1/plugin/disable', {'install_id': iid})['plugin']['status']
        require(disabled['install_id'] == iid and disabled['enabled'] is False,
                'Plugin publication did not disable the selected installation')
        filters = {'kind': None, 'status': 'disabled', 'source_id': source_id, 'include_disabled': True}
        listing = call('/v1/plugin/list', filters)
        require(any(entry['status'].get('install_id') == iid for entry in listing['plugins']),
                'Active Worker did not list the published disabled installation')
        other = call('/v1/mcp/servers/import', {
            'json': json.dumps({'command': sys.executable, 'args': [str(Path(__file__).with_name('mcp_stdio.py'))]}),
            'name': 'isolated', 'enable': False, 'conflictPolicy': 'reject',
        }, node=b)
        other_install = Path(other['install']['installed_path'])
        require(other_install.is_relative_to(b['home']) and other_install.exists(),
                'Other Worker did not publish to its own filesystem')
        require(not any(entry['id'] == other['plugin']['id'] for entry in
                        call('/v1/plugin/list', {**filters, 'status': None, 'source_id': None})['plugins']),
                'Other Worker publication leaked into the active Worker catalog')
        require(cluster.query('SELECT turn_id FROM agent_active_turns WHERE session_id=?', (session,)) == [(turn_id,)],
                'Plugin publications did not overlap the active Turn')
    finally:
        release.touch()
    events = cluster.events('alice', session, turn_id, 90)
    require(completed.exists() and events[-1]['payload']['type'] == 'turnCompleted'
            and 'PLUGIN_GATE_OK' in final_answer(events),
            'Plugin publication disrupted the active Turn')
    passed('plugin_publication_during_active_turn_is_isolated_per_worker')

    cluster.wait_worker_settlement('alice-a')
    cluster.stop(a['process'])
    wait_for(lambda: not cluster.info('alice-a').get('available'), 'Worker disconnect')
    cluster.start_worker('alice-a')
    wait_for(lambda: cluster.info('alice-a').get('available'), 'Worker restart')
    require(any(s['id']==source_id for s in query('/v1/plugin/sources')['sources']), 'UUID restart lost source')
    require(call('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='credentialsStored','Restart lost credential')
    call('/v1/mcp/servers/credentials',{'server':owner,'change':{'type':'clear'}},method='PUT')
    call('/v1/plugin/uninstall',{'install_id':mcp['install']['id']})
    call('/v1/plugin/uninstall',{'install_id':iid})
    require(not Path(install['installed_path']).exists(), 'Uninstall left artifact')
    call('/v1/plugin/sources/remove',{'source_id':source_id})
    require(not any(s['id']==source_id for s in query('/v1/plugin/sources')['sources']), 'Remove left source')
    cluster.stop(a['process'])
    wait_for(lambda: not cluster.info('alice-a').get('available'), 'Worker offline')
    failure=http(cluster.url+'/v1/plugin/sources?'+urlencode({'nodeId':a['node_id']}),token=token)
    require(failure.get('error',{}).get('code') in ('serviceUnavailable','nodeUnavailable'), 'Offline catalog fell back to embedded')
    passed('restart_preserves_identity_and_environment_then_cleanup_and_offline_are_explicit')
