"""Real public relay callbacks, SDK PKCE, credentials and MCP execution on outbound Worker."""
from pathlib import Path
import json
import os
import ssl
import sys
import subprocess
import concurrent.futures
import time
import urllib.error
import urllib.request
from urllib.parse import urlparse,parse_qs,urlencode
import yaml
from fixture import ROOT,free_port,data,http,require,wait_for
from scenarios import passed
from configuration import ConfigurationCluster
from registration import start_ingress


class OAuthCluster(ConfigurationCluster):
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        self.ingress=f'https://127.0.0.1:{free_port()}'
    def start_control_plane(self):
        super().start_control_plane()
        start_ingress(self)

    def config(self,home,node_config):
        env=super().config(home,node_config)
        path=Path(env['TALOS_CONFIG'])
        config=yaml.safe_load(path.read_text())
        config['cloud']['agent_base_url']=self.ingress
        path.write_text(yaml.safe_dump(config,sort_keys=False))
        return env


def oauth(cluster):
    token=cluster.users['alice']['access_token']
    a=cluster.nodes['alice-a']
    peer_home=cluster.root/'oauth-issuer'
    peer_home.mkdir()
    peer=cluster.start_process('oauth-issuer',[sys.executable,str(ROOT/'talos-rs/crates/action/mcp/tests/fixtures/oauth_server.py'),str(peer_home)])
    wait_for(lambda:(peer_home/'port').exists(),'Actual OAuth issuer ready')
    issuer='http://127.0.0.1:'+ (peer_home/'port').read_text()
    def api(path,body,node=a):
        return data(http(cluster.url+path,{**body,'nodeId':node['node_id']},token))
    # Keep this second Worker connected for the actual ten-minute attempt lifetime.
    b=cluster.nodes['alice-b']
    expiry_install=api('/v1/mcp/servers/import',{'json':json.dumps({'name':'oauth_expiry','url':issuer+'/bearer'}),'enable':True,'conflictPolicy':'reject'},node=b)
    expiry_owner={'type':'plugin','installId':expiry_install['install']['id']}
    expiry=api('/v1/mcp/servers/auth/start',{'server':expiry_owner,'config':{'scopes':['read']}},node=b)
    expiry_state=parse_qs(urlparse(expiry['authorizationUrl']).query)['state'][0]
    installed=api('/v1/mcp/servers/import',{'json':json.dumps({'name':'remote_oauth','url':issuer+'/bearer'}),'enable':True,'conflictPolicy':'reject'})
    owner={'type':'plugin','installId':installed['install']['id']}
    started=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    params=parse_qs(urlparse(started['authorizationUrl']).query)
    require(params['redirect_uri']==[cluster.ingress+'/v1/mcp/oauth/callback'],'Grant points at Worker loopback instead of public control plane')
    listeners=subprocess.run(['lsof','-nP','-a','-p',str(a['process'].pid),'-iTCP','-sTCP:LISTEN'],capture_output=True,text=True)
    require(listeners.returncode==0 and listeners.stdout,'Worker RPC listener is missing')
    require(all('TCP 127.0.0.1:' in line for line in listeners.stdout.splitlines()[1:]),'Remote OAuth opened a non-loopback Worker listener')
    require(not list((a['home']/'mcp-credentials').glob('*.json')), 'Starting authorization persisted tokens before callback')
    passed('remote_grant_uses_public_relay_and_loopback_worker_rpc')

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self,req,fp,code,msg,headers,newurl): return None
    browser=urllib.request.build_opener(urllib.request.ProxyHandler({}),NoRedirect(),urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=cluster.cert)))
    try: browser.open(started['authorizationUrl'],timeout=15)
    except urllib.error.HTTPError as response:
        require(response.code==302,'Issuer did not issue actual browser grant')
        callback=response.headers['Location']
    callback_params={key:values[0] for key,values in parse_qs(urlparse(callback).query).items()}
    def deliver(values):
        request=urllib.request.Request(cluster.ingress+'/v1/mcp/oauth/callback?'+urlencode(values))
        try: response=browser.open(request,timeout=60)
        except urllib.error.HTTPError as response_error: response=response_error
        with response:
            body=response.read()
            require(response.headers.get('Cache-Control')=='no-store' and response.headers.get('Referrer-Policy')=='no-referrer','Callback leaks response or referrer data')
            require(callback_params['state'].encode() not in body and callback_params['code'].encode() not in body,'Callback echoed secrets')
            return response.status,body
    require(deliver(callback_params)[0]==200,'Actual public callback did not complete grant')
    require(api('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='authenticated','Callback delivery was mistaken for authentication')
    require(deliver(callback_params)[0]==404,'Repeated callback exchanged code again')
    bundles=list((a['home']/'mcp-credentials').glob('*.json'))
    require(any('private-refresh' in path.read_text() for path in bundles),'OAuth bundle not saved on target Worker')
    require(not any('private-refresh' in path.read_text() for path in (cluster.server_home/'mcp-credentials').glob('*.json')),'OAuth token copied to control plane')
    require(not any('private-refresh' in path.read_text() for path in (cluster.nodes['alice-b']['home']/'mcp-credentials').glob('*.json')),'OAuth token copied to another Worker')
    passed('public_callback_commits_once_and_secrets_remain_on_original_worker')

    session,ack=cluster.start_turn('alice','Call ToolSearch with exact query mcp__remote_oauth__inspect, then call that exact MCP tool. Reply REMOTE_OAUTH_TOOL_OK.',node_id=a['node_id'])
    events=cluster.events('alice',session,data(ack)['turnId'],120)
    require(events[-1]['payload']['type']=='turnCompleted' and (peer_home/'called').exists(),'Real authorized MCP tool did not execute')
    # Stop only after the Worker has received the terminal cursor acknowledgement.
    # A terminal SSE fact precedes final RPC cleanup/commit, so restarting here
    # without observing handoff readiness can interrupt that legitimate settlement.
    markers = a['home'] / 'data/worker/session-heads'
    wait_for(lambda: bool(list(markers.glob('*.json'))) and all(
        (lambda marker: marker['active'] is None and marker['cleanup_confirmed'])(json.loads(path.read_text()))
        for path in markers.glob('*.json')), 'Worker terminal cursor committed before restart', 40)

    cluster.stop(a['process']);cluster.start_worker('alice-a')
    wait_for(lambda:cluster.info('alice-a').get('available'),'OAuth Worker UUID restart')
    require(api('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='authenticated','Restart lost OAuth credential')
    passed('real_mcp_tool_and_restart_use_worker_owned_oauth_bundle')

    first=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    second=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    first_state=parse_qs(urlparse(first['authorizationUrl']).query)['state'][0]
    require(deliver({'state':first_state,'code':'unused','iss':issuer})[0]==404,'Superseded route remained usable')
    second_state=parse_qs(urlparse(second['authorizationUrl']).query)['state'][0]
    cluster.stop(a['process'])
    wait_for(lambda:not cluster.info('alice-a').get('available'),'OAuth connection withdrawn')
    require(deliver({'state':second_state,'code':'unused','iss':issuer})[0] in (404,503),'Disconnected attempt was redirected to another connection')
    cluster.start_worker('alice-a');wait_for(lambda:cluster.info('alice-a').get('available'),'OAuth reconnect')
    require(api('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='authenticated','Uncompleted attempt overwrote original credential')
    passed('superseded_and_disconnected_callbacks_never_replay_or_change_credentials')

    pending=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    pending_state=parse_qs(urlparse(pending['authorizationUrl']).query)['state'][0]
    api('/v1/plugin/disable',{'install_id':installed['install']['id']})
    require(deliver({'state':pending_state,'code':'unused','iss':issuer})[0]!=200,'Disabled installation accepted pending authorization')
    api('/v1/plugin/enable',{'install_id':installed['install']['id']})
    require(api('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='authenticated','Disable destroyed stored credentials')
    passed('disabling_a_service_cancels_pending_grants_without_removing_stored_credentials')

    pending=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    try: browser.open(pending['authorizationUrl'],timeout=15)
    except urllib.error.HTTPError as response:
        require(response.code==302,'Issuer grant failed')
        interrupted={key:values[0] for key,values in parse_qs(urlparse(response.headers['Location']).query).items()}
    before=int((peer_home/'exchange-count').read_text())
    (peer_home/'hold-exchange').write_text('hold actual exchange')
    with concurrent.futures.ThreadPoolExecutor(max_workers=1) as executor:
        delivered=executor.submit(deliver,interrupted)
        wait_for(lambda:int((peer_home/'exchange-count').read_text())==before+1,'Actual code exchange accepted once')
        cluster.stop(a['process'])
        wait_for(lambda:not cluster.info('alice-a').get('available'),'Disconnect during exchange')
        require(delivered.result(timeout=60)[0]!=200,'Disconnected grant falsely reported completion')
        (peer_home/'hold-exchange').unlink()
    cluster.start_worker('alice-a');wait_for(lambda:cluster.info('alice-a').get('available'),'Reconnect after uncertain exchange')
    require(int((peer_home/'exchange-count').read_text())==before+1,'Reconnect replayed authorization code')
    require(api('/v1/mcp/servers/auth/read',{'server':owner})['state']['type']=='authenticated','Interrupted grant overwrote prior bundle')
    passed('disconnect_during_real_exchange_never_replays_or_publishes_the_interrupted_grant')

    # A real lifetime observation avoids shortening production expiry or mutating database state.
    remaining=expiry['expiresAt']-time.time()+1
    if remaining>0:
        print('Waiting for the original OAuth attempt deadline; heartbeat and renewal remain active.',flush=True)
        while time.time()<expiry['expiresAt']+1:
            time.sleep(min(10,expiry['expiresAt']+1-time.time()))
    require(api('/v1/mcp/servers/auth/read',{'server':expiry_owner},node=b)['state']['type']=='expired','Original Worker attempt did not expire')
    require(deliver({'state':expiry_state,'code':'unused','iss':issuer})[0]==404,'Expired routing forwarded code')
    require(not list((b['home']/'mcp-credentials').glob('*.json')),'Expired attempt persisted secrets')
    require(cluster.info('alice-b').get('available'),'Waiting OAuth attempt broke registration or renewal')
    passed('original_ten_minute_deadline_expires_worker_attempt_and_control_plane_route')

    pending=api('/v1/mcp/servers/auth/start',{'server':owner,'config':{'scopes':['read']}})
    pending_state=parse_qs(urlparse(pending['authorizationUrl']).query)['state'][0]
    data(http(cluster.url+'/v1/auth/logout',{},token))
    require(deliver({'state':pending_state,'code':'unused','iss':issuer})[0]==404,'Logout preserved initiating actor callback policy')
    passed('logout_removes_callback_policy_without_copying_or_exposing_credentials')
