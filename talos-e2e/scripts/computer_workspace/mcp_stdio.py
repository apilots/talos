"""A real stdio MCP service used to prove node-local credentials and tool effects."""
import json
import os
from pathlib import Path
import sys

for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'initialize':
        result = {'protocolVersion': request['params']['protocolVersion'], 'capabilities': {'tools': {}}, 'serverInfo': {'name': 'computer-evidence', 'version': '1.0'}}
    elif method == 'tools/list':
        result = {'tools': [{'name': 'record_evidence', 'description': 'Record actual execution evidence on this Computer.', 'inputSchema': {'type': 'object', 'properties': {}, 'additionalProperties': False}}]}
    elif method == 'tools/call':
        Path(os.environ['EVIDENCE_FILE']).write_text(os.environ['PRIVATE_KEY'])
        result = {'content': [{'type': 'text', 'text': 'COMPUTER_MCP_EXECUTED'}]}
    elif method == 'ping':
        result = {}
    else:
        print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'error': {'code': -32601, 'message': 'Unknown fixture method'}}), flush=True)
        continue
    print(json.dumps({'jsonrpc': '2.0', 'id': request['id'], 'result': result}), flush=True)
