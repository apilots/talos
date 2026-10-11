"""Node-local document, Memory index and Skills acceptance through public APIs."""
from pathlib import Path
from urllib.parse import urlencode
import uuid
import yaml
from fixture import data, http, require, wait_for
from scenarios import passed
from workspace import WorkspaceCluster


class EnvironmentCluster(WorkspaceCluster):
    def config(self, home, node_config):
        env = super().config(home, node_config)
        if 'runner' in node_config:
            skill = home / 'test-skills' / 'computer-review'
            skill.mkdir(parents=True)
            (skill / 'SKILL.md').write_text(
                '---\nname: computer-review\ndescription: Review this Computer.\n---\n\n'
                f'The Computer review marker is SKILL_{home.name}.\n')
            path = Path(env['TALOS_CONFIG'])
            config = yaml.safe_load(path.read_text())
            config['agent']['kernel']['plugins']['roots'] = [
                {'kind': 'skill', 'path': str(skill.parent), 'read_only': True}]
            path.write_text(yaml.safe_dump(config, sort_keys=False))
        return env


def memory_document(workspace_id, memory_id, marker):
    # Canonical existing OKF/Dream Memory fields; all mutation goes through the API.
    return f'''---
type: DreamMemory
title: Dream Memory 2026-10-11
description: Durable memories formed for 2026-10-11.
tags: [talos, dream-memory]
status: stable
generated: {{ by: talos-agent/{workspace_id}, at: 2026-10-11T12:00:00Z }}
---

# Dream Memory 2026-10-11

<!-- talos:dream-memory:start -->

## Memories

<!-- talos:dream-memory:entry key="{memory_id}" -->
### Computer evidence

- **Occurred At:** 1791720000
- **Kind:** fact
- **Importance:** normal

{marker}

<!-- talos:dream-memory:end -->
'''


def environment(cluster):
    token = cluster.users['alice']['access_token']
    server_workspace = cluster.server_home / 'workspace-talos'
    # The Server's own embedded Worker may already have its bootstrap templates.
    # Remote operations must neither add to nor change those existing files.
    server_baseline = {str(path.relative_to(server_workspace)): path.read_bytes() for path in server_workspace.rglob('*') if path.is_file()}
    nodes = [cluster.nodes[name] for name in ('alice-a', 'alice-b')]
    memory_id = 'mem_' + uuid.uuid4().hex
    documents = []
    for node in nodes:
        target = {'nodeId': node['node_id']}
        query = urlencode(target)
        info = data(http(f'{cluster.url}/v1/workspace/info?{query}', token=token))
        marker = 'DOC_' + node['home'].name
        written = data(http(f'{cluster.url}/v1/workspace/docs/soul?{query}', {
            'content': f'The Computer document marker is {marker}.',
            'base_version': {'mode': 'must_not_exist'}}, token, method='PUT'))['doc']
        read = data(http(f'{cluster.url}/v1/workspace/docs/soul?{query}', token=token))['doc']
        require(read == written, 'Document write/read changed its version or fields')
        summaries = data(http(f'{cluster.url}/v1/workspace/docs?{query}', token=token))['data']
        require(next(doc for doc in summaries if doc['kind'] == 'soul')['version_token'] == written['version_token'], 'Document list lost its version')
        stale = http(f'{cluster.url}/v1/workspace/docs/soul?{query}', {
            'content': 'stale overwrite', 'base_version': {'mode': 'match', 'version_token': 'stale'}}, token, method='PUT')
        require(stale.get('error', {}).get('code') == 'conflict', 'Stale document version overwrote Worker data')
        root = Path(info['userWorkspaceDir'])
        require(marker in (root / 'SOUL.md').read_text(), 'Document was not saved on selected Worker')
        documents.append((query, root, marker, read))

        content = memory_document(info['workspaceId'], memory_id, 'MEMORY_' + node['home'].name)
        body = {**target, 'date': '2026-10-11', 'content': content, 'expected_version': None}
        file = data(http(f'{cluster.url}/v1/memory/files/2026-10-11', body, token, method='PUT'))['file']
        require(data(http(f'{cluster.url}/v1/memory/files/2026-10-11?{query}', token=token))['file'] == file, 'Memory file read used another node')
        require(data(http(f'{cluster.url}/v1/memory/files?{query}', token=token))['data'][0]['version_token'] == file['version_token'], 'Memory listing ignored node')
        search = urlencode({**target, 'query': 'MEMORY_' + node['home'].name, 'kinds': 'fact'})
        hits = data(http(f'{cluster.url}/v1/memory/search?{search}', token=token))['data']
        require(len(hits) == 1 and hits[0]['memoryId'] == memory_id, 'Worker index did not include API write')
        entry = data(http(f'{cluster.url}/v1/memory/entries/{memory_id}?{query}', token=token))['memory']
        require(entry['body'] == 'MEMORY_' + node['home'].name, 'Memory ID resolved on another Computer')
        stale = http(f'{cluster.url}/v1/memory/files/2026-10-11', {**body, 'content': content+'\n', 'expected_version': 'stale'}, token, method='PUT')
        require(stale.get('error', {}).get('code') == 'conflict', 'Stale Memory update overwrote data')
        changed = content.replace('MEMORY_', 'UPDATED_')
        updated = data(http(f'{cluster.url}/v1/memory/files/2026-10-11', {**body, 'content': changed, 'expected_version': file['version_token']}, token, method='PUT'))['file']
        require(updated['version_token'] != file['version_token'], 'Versioned Memory write did not advance')
        require(not data(http(f'{cluster.url}/v1/memory/search?{search}', token=token))['data'], 'Memory index retained stale body')
        require('UPDATED_' in (root / 'memory/2026-10-11.md').read_text(), 'Memory file saved outside Worker')

        skills = data(http(f'{cluster.url}/v1/skills?{query}', token=token))
        require([skill['name'] for skill in skills['data']] == ['computer-review'], 'Worker Skills root was not selected')
        require(data(http(f'{cluster.url}/v1/skills/refresh?{query}', {}, token, method='POST')) == skills, 'Skills refresh changed target')
        suggestions = data(http(f'{cluster.url}/v1/skills/suggest?{urlencode({**target, "query": "computer", "limit": 1})}', token=token))['data']
        require(len(suggestions) == 1, 'Worker Skills suggestions missing')
        read = data(http(f'{cluster.url}/v1/skills/read', {**target, 'selector': {'name': 'computer-review'}}, token))['document']
        require('SKILL_' + node['home'].name in read['contents'], 'Skill document came from another node')
        resolved = data(http(f'{cluster.url}/v1/skills/resolve', {**target, 'text': 'Use $computer-review', 'selected': []}, token))
        require(resolved['data'] == skills['data'] and not resolved['warnings'], 'Skill resolution diverged')
    passed('two_workers_isolate_documents_memory_ids_indexes_and_skills')
    passed('version_conflicts_and_index_replacement_preserve_existing_semantics')

    for node, (_, _, marker, _) in zip(nodes, documents):
        _, answer, _ = cluster.turn('alice', 'Read the Computer document marker from your SOUL instructions and reply with just that marker.', node_id=node['node_id'])
        require(marker in answer, 'Harness did not use the Worker-managed document')
    passed('real_harness_consumes_selected_worker_documents')
    server_after = {str(path.relative_to(server_workspace)): path.read_bytes() for path in server_workspace.rglob('*') if path.is_file()}
    changed = sorted(path for path in server_baseline.keys() | server_after.keys() if server_baseline.get(path) != server_after.get(path))
    require(not changed, f'Remote operations changed control-plane Worker files: {changed[:20]}')
    denied = http(f'{cluster.url}/v1/workspace/docs?{documents[0][0]}', token=cluster.users['bob']['access_token'])
    require(denied.get('error', {}).get('code') == 'permissionDenied', 'Foreign owner accessed environment')
    passed('control_plane_only_routes_metadata_and_rejects_foreign_owner')

    node = nodes[0]
    cluster.stop(node['process'])
    query, root, _, saved = documents[0]
    for path in ('workspace/docs/soul', 'memory/files', 'skills'):
        failed = http(f'{cluster.url}/v1/{path}?{query}', token=token)
        require(failed.get('error', {}).get('code') == 'serviceUnavailable', 'Offline node used control-plane environment')
    require((root / 'SOUL.md').read_text() == saved['content'], 'Offline read modified node data')
    cluster.start_worker('alice-a')
    wait_for(lambda: cluster.info('alice-a').get('available'), 'same UUID environment restart')
    require(data(http(f'{cluster.url}/v1/workspace/docs/soul?{query}', token=token))['doc'] == saved, 'Restart lost document/version')
    hits = data(http(f'{cluster.url}/v1/memory/search?{urlencode({"nodeId": node["node_id"], "query": "UPDATED_"})}', token=token))['data']
    require(hits and hits[0]['memoryId'] == memory_id, 'Worker restart lost/reused another index')
    passed('offline_rejection_and_restart_restore_the_original_worker_files')
