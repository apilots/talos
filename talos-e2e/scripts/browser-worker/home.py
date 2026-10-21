"""Real packaged home, zero-window desktop and same-process launcher acceptance."""
import json
import re
import subprocess
import time
import uuid
from media import Viewer

WORKSPACE = '/home/talos/.talos/browser/workspace'

def is_home_url(value):
    match = re.fullmatch(r'http://127\.0\.0\.1:([0-9]+)/home\.html', value)
    return bool(match and 0 < int(match[1]) < 65536)

def desktop_windows(container):
    code = '''import json,pathlib,re,subprocess
root=subprocess.check_output(['xprop','-root','_NET_CLIENT_LIST'],text=True)
windows=[]
for window in re.findall(r'0x[0-9a-fA-F]+',root):
 try:
  properties=subprocess.check_output(['xprop','-id',window,'WM_CLASS','WM_NAME','_NET_WM_NAME','_NET_WM_PID','_NET_FRAME_EXTENTS'],text=True,stderr=subprocess.DEVNULL)
  match=re.search(r'_NET_WM_PID\\(CARDINAL\\) = (\\d+)',properties)
  pid=int(match[1]) if match else None
  uid=int((pathlib.Path('/proc')/str(pid)/'status').read_text().split('Uid:')[1].split()[0]) if pid else None
  geometry=subprocess.check_output(['xwininfo','-id',window],text=True)
  bounds={key:int(re.search(label+r':\\s+(-?\\d+)',geometry)[1]) for key,label in [('x','Absolute upper-left X'),('y','Absolute upper-left Y'),('width','Width'),('height','Height')]}
  frame=re.search(r'_NET_FRAME_EXTENTS\\(CARDINAL\\) = (\\d+), (\\d+), (\\d+), (\\d+)',properties)
  windows.append({'id':window,'properties':properties,'uid':uid,'pid':pid,'frameTop':int(frame[3]) if frame else 0,**bounds})
 except (subprocess.CalledProcessError,FileNotFoundError):pass
print(json.dumps(windows))'''
    return json.loads(subprocess.check_output(['docker','exec','-e','DISPLAY=:99',
        '-e','XAUTHORITY=/home/talos/.talos/browser/Xauthority',container,'python3','-c',code],text=True))

def desktop_active(container,window):
    output=subprocess.check_output(['docker','exec','-e','DISPLAY=:99',
        '-e','XAUTHORITY=/home/talos/.talos/browser/Xauthority',container,
        'xprop','-root','_NET_ACTIVE_WINDOW'],text=True)
    return int(output.rsplit(' ',1)[-1],16)==int(window['id'],16)

def window_focus_roundtrip(viewer,container,wait,windows):
    # Arrange three real client windows so each remains clickable after another
    # is raised. Test setup uses X configure requests; focus/input use only RFB.
    code='''import ctypes,json,sys
x=ctypes.CDLL('libX11.so.6');x.XOpenDisplay.restype=ctypes.c_void_p
d=x.XOpenDisplay(None);assert d
x.XMoveResizeWindow.argtypes=[ctypes.c_void_p,ctypes.c_ulong,ctypes.c_int,ctypes.c_int,ctypes.c_uint,ctypes.c_uint]
x.XSync.argtypes=[ctypes.c_void_p,ctypes.c_int]
for i,window in enumerate(json.loads(sys.argv[1])):x.XMoveResizeWindow(d,int(window,16),20+480*i,100,430,500)
x.XSync(d,False)'''
    subprocess.run(['docker','exec','-e','DISPLAY=:99','-e','XAUTHORITY=/home/talos/.talos/browser/Xauthority',
        container,'python3','-c',code,json.dumps([window['id'] for window in windows])],check=True)
    def current():return {window['id']:window for window in desktop_windows(container)}
    wait(lambda:all(abs(current()[window['id']]['x']-(20+480*i))<25 for i,window in enumerate(windows)),
        'Three real application windows arranged')
    for titlebar in [False,True]:
        for window in [*windows,windows[1]]:
            bounds=current()[window['id']]
            # Chromium draws its own titlebar (zero frame extents); never click
            # the wallpaper above that undecorated client window.
            title_y=bounds['y']-bounds['frameTop']//2 if bounds['frameTop'] else bounds['y']+10
            # Use the application content area: PCManFM's Places selector at
            # y+100 opens a GTK popup which correctly consumes the next click.
            client_x=bounds['x']+bounds['width']*3//4
            client_y=bounds['y']+bounds['height']*3//5
            viewer.click(bounds['x']+min(120,bounds['width']//2) if titlebar else client_x,
                title_y if titlebar else client_y)
            wait(lambda:desktop_active(container,window),f'{"Titlebar" if titlebar else "Client"} click focuses {window["id"]}')

def desktop_apps(viewer, container, wait, report):
    def named(name):
        return next((window for window in desktop_windows(container) if f'= "{name}"' in window['properties']),None)
    def files():
        return next((window for window in desktop_windows(container) if '"pcmanfm"' in window['properties'].lower()),None)
    def browser():
        return next((window for window in desktop_windows(container) if any(name in window['properties'].lower()
            for name in ['"google-chrome"','"chromium"','"chromium-browser"'])),None)
    def close():viewer.chord(0xffe9,0xffc1)  # Alt+F4, through the current controller.
    # Three 32px launchers in the packaged centered 180x56 panel.
    viewer.click(666,852)
    browser_window=wait(browser,'Browser dock entry')
    viewer.click(708,852)
    terminal=wait(lambda:named('Terminal'),'Terminal dock entry')
    assert terminal['uid']==10001,terminal
    viewer.text("printf 'desktop-terminal-evidence\\n' > desktop-note.txt")
    viewer.press(0xff0d)
    def created():
        result=subprocess.run(['docker','exec',container,'cat',WORKSPACE+'/desktop-note.txt'],capture_output=True,text=True)
        return result.returncode==0 and result.stdout=='desktop-terminal-evidence\n'
    wait(created,'File created by real terminal input')
    viewer.frame(report/'desktop-terminal.png')
    viewer.click(750,852)
    file_window=wait(files,'Files dock entry')
    assert file_window['uid']==10001,file_window
    window_focus_roundtrip(viewer,container,wait,[browser_window,terminal,file_window])
    # The final roundtrip click returns to Terminal. Prove keyboard input is
    # delivered there, not into the previously active Browser or Files window.
    viewer.text("printf 'focus-delivery-evidence\\n' > focus-note.txt")
    viewer.press(0xff0d)
    def delivered():
        result=subprocess.run(['docker','exec',container,'cat',WORKSPACE+'/focus-note.txt'],capture_output=True,text=True)
        return result.returncode==0 and result.stdout=='focus-delivery-evidence\n'
    wait(delivered,'Terminal receives keyboard input after application focus roundtrip')
    bounds=next(window for window in desktop_windows(container) if window['id']==file_window['id'])
    viewer.click(bounds['x']+bounds['width']*3//4,bounds['y']+bounds['height']*3//5)
    wait(lambda:desktop_active(container,file_window),'Files receives focus before file activation')
    viewer.frame(report/'desktop-files.png')
    # The fresh fixture has two sorted files, desktop-note then focus-note.
    # Select the first visible file and open it through PCManFM's normal MIME
    # action; its location bar accepts directories, not file activation.
    viewer.press(0xff50)  # Home selects the first item in the focused file view.
    viewer.press(0xff0d)
    preview=wait(lambda:named('File'),'Text opened from file manager')
    assert preview['uid']==10001,preview
    command=subprocess.check_output(['docker','exec',container,'cat',f'/proc/{preview["pid"]}/cmdline'])
    assert command.split(b'\0')[-2].decode()==WORKSPACE+'/desktop-note.txt',command
    viewer.frame(report/'desktop-file-preview.png')
    close();wait(lambda:not named('File'),'Text preview closed')
    close();wait(lambda:not files(),'File manager window closed')
    close();wait(lambda:not named('Terminal'),'Terminal window closed')
    close();wait(lambda:not browser(),'Browser window closed after focus roundtrip')
    viewer.click(708,852)
    reopened=wait(lambda:named('Terminal'),'Terminal reopened from dock')
    # X may reuse a retired client's window ID; process identity proves that
    # closing and launching the terminal created a new real application.
    assert reopened['pid']!=terminal['pid'] and reopened['uid']==10001,reopened
    close();wait(lambda:not named('Terminal'),'Reopened terminal closed')
    viewer.click(750,852)
    reopened=wait(files,'Files reopened from dock')
    assert reopened['uid']==10001,reopened
    close();wait(lambda:not files(),'Reopened file manager closed')


def desktop_home(url, node, container, http, data, wait, report):
    state = data(http(url+'/v1/browser', {'nodeId':node, 'operation':{'kind':'read'}}))
    viewer_id=str(uuid.uuid4())
    grant = data(http(url+'/v1/desktop/open', {'nodeId':node, 'targetId':'managed',
        'viewerId':viewer_id, 'generation':state['control']['generation'],
        'inputMode':'readOnly', 'controllerId':None, 'inputEpoch':None}))
    viewer = Viewer(url, grant)
    viewers=[viewer]
    def state():return data(http(url+'/v1/browser', {'nodeId':node, 'operation':{'kind':'read'}}))
    def inspect(close=False):
        code = '''import asyncio,json,pathlib,re,websockets
async def run():
 lines=pathlib.Path('/home/talos/.talos/browser/profile/DevToolsActivePort').read_text().splitlines()
 async with websockets.connect('ws://127.0.0.1:'+lines[0]+lines[1],origin=None) as ws:
  seq=0
  async def call(method,params=None,session=None):
   nonlocal seq
   seq+=1;request={'id':seq,'method':method,'params':params or {}}
   if session:request['sessionId']=session
   await ws.send(json.dumps(request))
   while True:
    result=json.loads(await ws.recv())
    if result.get('id')==seq:
     assert 'error' not in result,result
     return result.get('result',{})
  pages=[p for p in (await call('Target.getTargets'))['targetInfos'] if p['type']=='page']
  if CLOSE:
   for page in pages:assert (await call('Target.closeTarget',{'targetId':page['targetId']}))['success']
   for attempt in range(40):
    pages=[p for p in (await call('Target.getTargets'))['targetInfos'] if p['type']=='page']
    if not pages:break
    await asyncio.sleep(.05)
   assert not pages,'Closed windows did not retire'
  result={'port':lines[0],'pages':[p['url'] for p in pages]}
  if pages:
   target=next(p for p in pages if re.fullmatch(r'http://127\\.0\\.0\\.1:[0-9]+/home\\.html',p['url']))
   result['homeUrl']=target['url']
   session=(await call('Target.attachToTarget',{'targetId':target['targetId'],'flatten':True}))['sessionId']
   result['home']=(await call('Runtime.evaluate',{'expression':'document.querySelector("h1")?.textContent','returnByValue':True},session))['result'].get('value')
   result['clock']=(await call('Runtime.evaluate',{'expression':'document.querySelector("#clock")?.textContent','returnByValue':True},session))['result'].get('value')
  print(json.dumps(result))
asyncio.run(run())'''.replace('CLOSE', str(close))
        return json.loads(subprocess.check_output(['docker','exec',container,
            '/opt/talos/browser/venv/bin/python','-c',code], text=True))
    def loaded():
        value = inspect()
        return value if value.get('home')=='Ready when you are.' and value.get('clock') not in (None,'--:--') else None
    try:
        before = wait(loaded, 'Packaged browser home')
        assert before['pages']==[before['homeUrl']] and is_home_url(before['homeUrl']), before
        assert before['home']=='Ready when you are.', before
        assert before['clock'] and before['clock']!='--:--', before
        viewer.frame()
        closed = inspect(close=True)
        assert closed['pages']==[] and closed['port']==before['port'], closed
        # Same admitted RFB socket still paints the neutral X desktop with no browser window.
        viewer.frame()
        environment_code="""import pathlib,json
result={}
for p in pathlib.Path('/proc').iterdir():
 if not p.name.isdigit():continue
 try:
  name=(p/'comm').read_text().strip()
  if name in ['openbox','tint2']:
   env=dict(entry.split('=',1) for entry in (p/'environ').read_text().split('\\0') if '=' in entry)
   result[name]={key:env.get(key) for key in ['DISPLAY','XAUTHORITY','TALOS_BROWSER_PROFILE','TALOS_BROWSER_HOME_URL']}
 except (FileNotFoundError,PermissionError,ProcessLookupError):pass
print(json.dumps(result))"""
        environments={}
        def owners_ready():
            nonlocal environments
            environments=json.loads(subprocess.check_output(['docker','exec',container,'python3','-c',environment_code],text=True))
            expected={'DISPLAY':':99','XAUTHORITY':'/home/talos/.talos/browser/Xauthority','TALOS_BROWSER_PROFILE':'/home/talos/.talos/browser/profile','TALOS_BROWSER_HOME_URL':before['homeUrl']}
            # A missing /proc entry is only retried until the bounded deadline;
            # a running owner with the wrong environment is always a failure.
            for process in ['openbox','tint2']:
                if process in environments:assert environments[process]==expected,environments
            return all(process in environments for process in ['openbox','tint2'])
        try:wait(owners_ready,'Actual desktop launcher owners',seconds=5)
        except AssertionError as error:
            raise AssertionError(f'Desktop launcher owners did not become ready: {environments}') from error
        snapshot=state()
        handoff=data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'takeControl','expectedRevision':snapshot['control']['revision']}}))
        control_grant=data(http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':viewer_id,
            'generation':handoff['control']['generation'],'inputMode':'control',
            'controllerId':handoff['control']['owner']['controllerId'],'inputEpoch':handoff['control']['epoch']}))
        viewer=Viewer(url,control_grant);viewers.append(viewer)
        wait(lambda:state()['control']['owner']['kind']=='human','Home control handshake')
        # Exercise the actual Openbox shortcut through authenticated RFB input.
        viewer.key(0xffeb,True);viewer.press(ord('b'));viewer.key(0xffeb,False)
        reopened=wait(loaded,'Openbox browser shortcut')
        assert reopened['port']==before['port'] and reopened['homeUrl']==before['homeUrl'] and reopened['home']==before['home'],reopened
        inspect(close=True);viewer.frame(report/'desktop-before-dock.png')
        desktop_apps(viewer,container,wait,report)
        # Fixed packaged 1440x900 display / centered 180x56 dock, first icon.
        viewer.click(666,852)
        reopened=wait(loaded,'Dock browser launcher')
        assert reopened['port']==before['port'] and reopened['homeUrl']==before['homeUrl'] and reopened['home']==before['home'],reopened
        viewer.frame()
        current=state()
        data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'releaseControl',
            'expectedRevision':current['control']['revision'],'controllerId':current['control']['owner']['controllerId']}}))
        (report/'desktop-home-acceptance.json').write_text(json.dumps({'status':'passed',
            'offlineHome':True,'loopbackHttpHome':before['homeUrl'],'liveClock':True,'zeroWindowsKeepsCdp':True,
            'sameRfbStreamPaintsDesktop':True,'launcherReusesProfileAndProcess':True,'realOpenboxShortcut':True,'realDockPointer':True,'actualLauncherOwnerEnvironments':True,
            'threeRealLaunchers':True,'terminalCreatedFile':True,'fileManagerOpenedFile':True,'applicationsRunAsUid10001':True,'terminalAndFilesReopened':True,
            'threeApplicationClientAndTitlebarFocus':True,'terminalKeyboardDeliveredAfterFocusRoundtrip':True},indent=2))
        print('PASS offline home / zero-window desktop / same supervised browser and profile reopened from launcher',flush=True)
    except Exception as error:
        diagnostic={'error':repr(error)}
        try:diagnostic['browser']=state()
        except Exception as observed:diagnostic['browserObservationError']=repr(observed)
        try:diagnostic['windows']=desktop_windows(container)
        except Exception as observed:diagnostic['windowObservationError']=repr(observed)
        (report/'desktop-home-failure.json').write_text(json.dumps(diagnostic,indent=2))
        try:viewer.frame(report/'desktop-home-failure.png')
        except Exception:pass
        raise
    finally:
        for viewer in viewers:viewer.close()
        # Let Worker/ingress retire this initial viewer before capacity acceptance starts.
        time.sleep(.2)
