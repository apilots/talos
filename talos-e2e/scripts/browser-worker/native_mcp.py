import importlib.util,json,os,subprocess,threading,uuid
from pathlib import Path
from lifecycle import own_process
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from native_engine import owned_chrome, stop_owned
ROOT=Path(__file__).resolve().parents[3]
spec=importlib.util.spec_from_file_location('browser_fixture',ROOT/'talos-e2e/scripts/browser-worker/fixture.py')
fixture=importlib.util.module_from_spec(spec);spec.loader.exec_module(fixture)
mock=ThreadingHTTPServer(('127.0.0.1',0),fixture.Fixture)
threading.Thread(target=mock.serve_forever,daemon=True).start()
class Page(BaseHTTPRequestHandler):
 def log_message(self,*args):pass
 def do_GET(self):
  if self.path=='/report.csv':
   self.send_response(200);self.send_header('Content-Type','text/csv');self.send_header('Content-Disposition','attachment; filename="report.csv"');self.end_headers();self.wfile.write(b'name,value\nworker,1\n');return
  html=b'''<title>Native Worker fixture</title><input name="name"><button onclick="document.querySelector('p').textContent='Hello '+document.querySelector('input').value">Submit</button><p>Ready</p><input id="upload" type="file" onchange="document.querySelector('#uploaded').textContent=this.files[0].name"><p id="uploaded"></p><a href="/report.csv">Download</a>'''
  self.send_response(200);self.send_header('Content-Type','text/html');self.send_header('Content-Length',str(len(html)));self.end_headers();self.wfile.write(html)
page=ThreadingHTTPServer(('127.0.0.1',18080),Page);threading.Thread(target=page.serve_forever,daemon=True).start()
home=Path(os.environ['TALOS_E2E_TMP_ROOT'])/('mcp-home-'+uuid.uuid4().hex[:8]);home.mkdir()
config=fixture.config(home,fixture.free_port(),mock.server_port)
config['gateway']['auth']=False;config['agent_node']={}
config['cloud']['agent_base_url']='https://127.0.0.1:'+str(fixture.free_port())
conf=home/'talos.json';conf.write_text(json.dumps(config))
env={k:os.environ[k] for k in ['PATH','TMPDIR','SYSTEMROOT','DISPLAY','XAUTHORITY','XDG_RUNTIME_DIR'] if k in os.environ}
env.update(HOME=str(home),TALOS_HOME=str(home),TALOS_CONFIG=str(conf),TALOS_RESOURCE_DIR=str(ROOT),TALOS_SCENARIO_REPORT_DIR=str(fixture.REPORT),TALOS_APP_SERVER_BIN=os.environ.get('TALOS_E2E_APP_SERVER_BIN',str(ROOT/'talos-rs/target/debug/app-server')))
electron=ROOT/'talos-app/desktop/node_modules/electron/dist/Electron.app/Contents/MacOS/Electron'
if not electron.exists():electron=ROOT/'talos-app/desktop/node_modules/electron/dist/electron'
result_file=fixture.REPORT/'native-mcp-acceptance.json';result_file.unlink(missing_ok=True)
try:
 with owned_chrome(fixture.REPORT, home, env):
  native=subprocess.Popen([str(electron),str(Path(os.environ['TALOS_E2E_NATIVE_ROOT'])/'mcp-dist')],env=env,start_new_session=True)
  own_process(native)
  try:
   if native.wait(timeout=210):raise RuntimeError('Desktop MCP acceptance failed')
  finally:stop_owned(native)
  assert result_file.exists(),'Electron exited before completing native MCP acceptance'
  result=json.loads(result_file.read_text())
  assert result['status']=='passed' and result['selectedPreparedChromeAfterResume'],result
  for mode in ('managed','connected'):
   assert result['modes'][mode]['status']=='passed',result
   assert (fixture.REPORT/result['modes'][mode]['pageScreenshot']).is_file(),result
finally:page.shutdown();mock.shutdown()
