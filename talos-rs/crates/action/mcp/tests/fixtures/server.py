"""Local MCP peer for process exit, blocked discovery and in-flight call scenarios."""
import json
import os
import sys
import time

root, mode = sys.argv[1:]
with open(root + '.pid', 'w') as target:
    target.write(str(os.getpid()))
if mode == 'hung':
    time.sleep(300)
# More than a pipe's capacity: startup needs a concurrent stderr consumer.
sys.stderr.write('x' * (1024 * 1024))
sys.stderr.flush()
changed = False
exit_on_list = False

def send(value):
    print(json.dumps(dict(jsonrpc='2.0', **value)), flush=True)

for line in sys.stdin:
    request = json.loads(line)
    if 'id' not in request:
        continue
    method = request['method']
    if method == 'server/discover':
        send(dict(id=request['id'], error=dict(code=-32601, message='legacy peer')))
        continue
    if method == 'initialize':
        result = dict(protocolVersion='2025-11-25', capabilities=dict(tools=dict(listChanged=True), resources={}), serverInfo=dict(name='fixture', version='1'))
    elif method == 'tools/list':
        if mode == 'oversized':
            sys.stdout.write('x' * (9 * 1024 * 1024))
            sys.stdout.flush()
            time.sleep(300)
        if exit_on_list:
            os._exit(7)
        page = request.get('params', {}).get('cursor')
        if page is None:
            result = dict(tools=[dict(name='inspect', inputSchema=dict(type='object'))], nextCursor='second')
        else:
            result = dict(tools=[dict(name='changed' if changed else 'hold', inputSchema=dict(type='object'))])
    elif method == 'tools/call':
        tool = request['params']['name']
        args = request['params'].get('arguments', {})
        child = None
        if args.get('refresh_exit'):
            child = os.fork()
            if child == 0:
                time.sleep(300)
                os._exit(0)
            exit_on_list = True
        if tool == 'hold':
            open(root + '.holding', 'w').close()
            while not os.path.exists(root + '.release'):
                time.sleep(0.01)
        result = dict(content=[], structuredContent=dict(pid=os.getpid(), child=child, revision=os.environ.get('REVISION')))
        if args.get('refresh'):
            changed = True
        send(dict(id=request['id'], result=result))
        if args.get('refresh') or exit_on_list:
            send(dict(method='notifications/tools/list_changed'))
        continue
    elif method == 'resources/list':
        cursor = request.get('params', {}).get('cursor')
        result = dict(resources=[dict(uri='fixture:' + (cursor or 'first'), name='fixture')])
        if cursor is None:
            result['nextCursor'] = 'last'
    elif method == 'resources/templates/list':
        result = dict(resourceTemplates=[])
    elif method == 'resources/read':
        result = dict(contents=[
            dict(uri='fixture:first', mimeType='text/plain', text='resource text'),
            dict(uri='fixture:binary', mimeType='application/octet-stream', blob='AQI=')
        ])
    else:
        send(dict(id=request['id'], error=dict(code=-32601, message='unsupported')))
        continue
    send(dict(id=request['id'], result=result))
