"""Original Turn API helpers shared by observable acceptance scenarios."""
from cluster import data, http, require


def submit(cluster, host, node, session, prompt, **options):
    body = {'nodeId': node, 'sessionId': session, 'input': {'content': [{'type': 'text', 'text': prompt}]},
            'modelSelection': {'modelRef': {'provider': cluster.model['provider'], 'id': cluster.model['id']}, 'reasoningEffort': None},
            'userPermissionMode': 'fullAccess', **options}
    if host == 'G':
        return data(http(cluster.url + '/v1/turns/start', body, cluster.users['alice']['access_token']))
    return cluster.desktop(host, 'turn/start', body)


def settled(cluster, host, session, turn):
    history = cluster.history(host, session)['data']
    result = next((h['result'] for h in history if h['identity']['turnId'] == turn), None)
    if result is None:
        return False
    require(result['status'] == 'completed', f'Turn failed: {result.get("error", result["status"])}')
    return history


def comparable_history(turns):
    """Compare execution facts without each coordinator's local Timeline cursor."""
    return [{key: value for key, value in turn.items() if key != 'completedEventSeq'} for turn in turns]
