"""Real same-origin WebSocket/RFB acceptance, without any Worker published port."""
import json
import struct
import subprocess
import time
import uuid
import zlib
from websockets.sync.client import connect
from websockets.exceptions import ConnectionClosed, InvalidStatus

class Viewer:
    def __init__(self,url,grant,token="browser-contract-alice",max_queue=16):
        self.pending=bytearray()
        self.socket=connect(url.replace('http','ws',1)+'/v1/desktop/stream',origin=url,proxy=None,open_timeout=5,max_size=65536,max_queue=max_queue)
        self.socket.send(json.dumps({'ticket':grant['ticket'],'accessToken':token}))
        self.grant=grant
        assert self.read(12)==b'RFB 003.008\n'
        self.socket.send(b'RFB 003.008\n')
        count=self.read(1)[0]
        assert 1 in self.read(count)
        self.socket.send(b'\x01')
        assert self.read(4)==b'\0\0\0\0'
        self.socket.send(b'\x01')
        initialization=self.read(24)
        self.width,self.height=struct.unpack('>HH',initialization[:4])
        assert self.width<=1600 and self.height<=1000
        self.name=self.read(struct.unpack('>I',initialization[20:24])[0])
        self.socket.send(struct.pack('>BBHI',2,0,1,0))
    def read(self,size):
        while len(self.pending)<size:
            chunk=self.socket.recv(timeout=5)
            assert isinstance(chunk,bytes)
            self.pending.extend(chunk)
            assert len(self.pending)<8*1024*1024
        result=bytes(self.pending[:size]);del self.pending[:size]
        return result
    def frame(self,screenshot=None):
        pixels=bytearray(self.width*self.height*3) if screenshot else None
        self.socket.send(struct.pack('>BBHHHH',3,0,0,0,self.width,self.height))
        assert self.read(1)==b'\0'
        header=self.read(3)
        rectangles=struct.unpack('>H',header[1:])[0]
        size=0;diverse=False
        for _ in range(rectangles):
            x,y,w,h,encoding=struct.unpack('>HHHHi',self.read(12))
            assert encoding==0
            raw=self.read(w*h*4)
            if pixels is not None:
                for row in range(h):
                    source=raw[row*w*4:(row+1)*w*4]
                    rgb=bytearray(w*3);rgb[0::3]=source[2::4];rgb[1::3]=source[1::4];rgb[2::3]=source[0::4]
                    start=((y+row)*self.width+x)*3;pixels[start:start+w*3]=rgb
            diverse=diverse or len(set(raw))>2
            size+=len(raw)
        assert size>0 and diverse
        if pixels is not None:
            def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data)&0xffffffff)
            rows=b''.join(b'\0'+pixels[row*self.width*3:(row+1)*self.width*3] for row in range(self.height))
            screenshot.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',self.width,self.height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b''))
        return size
    def key(self,key,down):
        self.socket.send(struct.pack('>BBHI',4,int(down),0,key))
    def press(self,key):
        self.key(key,True);self.key(key,False)
    def click(self,x,y):
        self.socket.send(struct.pack('>BBHH',5,0,x,y))
        self.socket.send(struct.pack('>BBHH',5,1,x,y))
        self.socket.send(struct.pack('>BBHH',5,0,x,y))
    def text(self,text):
        assert text.isascii(), 'Fixture text must use ASCII keysyms'
        for character in text:self.press(ord(character))
    def chord(self,modifier,key):
        self.key(modifier,True);self.press(key);self.key(modifier,False)
    def close(self):
        self.socket.close()

def media(url,node,container,http,data,wait,report):
    def state():return data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'read'}}))
    def operate(kind,**values):
        return data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':kind,'expectedRevision':state()['control']['revision'],**values}}))
    def grant(viewer,snapshot=None,control=False):
        snapshot=snapshot or state();owner=snapshot['control']['owner']
        return data(http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':viewer,
            'generation':snapshot['control']['generation'],'inputMode':'control' if control else 'readOnly',
            'controllerId':owner['controllerId'] if control else None,'inputEpoch':snapshot['control']['epoch'] if control else None}))
    viewers=[]
    try:
        try:
            connect(url.replace('http','ws',1)+'/v1/desktop/stream',origin='https://attacker.example',proxy=None)
            raise AssertionError('Cross-origin viewer was admitted')
        except InvalidStatus as error:
            assert error.response.status_code==403
        viewer_id=str(uuid.uuid4())
        pending=grant(viewer_id)
        def denied(grant,token):
            with connect(url.replace('http','ws',1)+'/v1/desktop/stream',origin=url,proxy=None,open_timeout=5) as socket:
                socket.send(json.dumps({'ticket':grant['ticket'],'accessToken':token}))
                try:socket.recv(timeout=5);raise AssertionError('Unauthorized desktop sent data')
                except ConnectionClosed:pass
        denied(pending,'invalid-fixture-token')
        denied(pending,'browser-contract-bob')
        forbidden=http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':str(uuid.uuid4()),'generation':state()['control']['generation'],'inputMode':'readOnly','controllerId':None,'inputEpoch':None},token='browser-contract-bob')
        assert forbidden['status']=='error','Other account admitted an owned Computer'
        a=Viewer(url,grant(viewer_id));viewers.append(a)
        denied(a.grant,'browser-contract-alice')
        bytes_observed=a.frame()
        b=Viewer(url,grant(str(uuid.uuid4())));viewers.append(b)
        b.frame()
        refused=http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':str(uuid.uuid4()),
            'generation':state()['control']['generation'],'inputMode':'readOnly','controllerId':None,'inputEpoch':None})
        assert refused.get('error',{}).get('code')=='conflict'
        assert state()['control']['owner']['kind']=='idle'
        handoff=operate('takeControl')
        assert handoff['control']['owner']['kind']=='handingOver'
        control=Viewer(url,grant(viewer_id,handoff,True));viewers.append(control)
        wait(lambda:state()['control']['owner']['kind']=='human','Controller handshake')
        controller=state()['control']['owner']['controllerId']
        current=state();before=current['control'].copy()
        for _ in range(3):
            rejected=http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':viewer_id,
                'generation':before['generation'],'inputMode':'control','controllerId':str(uuid.uuid4()),'inputEpoch':before['epoch']-1})
            assert rejected.get('error',{}).get('code')=='permissionDenied', 'Failed Worker admission leaked an ingress reservation: '+json.dumps(rejected)
            assert state()['control']==before,'Rejected replacement changed the current controller'
        control.frame()  # The same original RFB socket must remain usable.
        control.key(0xffe1,True)
        time.sleep(.15)
        def shift():
            code="import ctypes,os; os.environ['DISPLAY']=':99'; os.environ['XAUTHORITY']='/home/talos/.talos/browser/Xauthority'; x=ctypes.CDLL('libX11.so.6'); x.XOpenDisplay.restype=ctypes.c_void_p; d=x.XOpenDisplay(None); assert d; x.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong]; k=x.XKeysymToKeycode(d,0xffe1); b=ctypes.create_string_buffer(32); x.XQueryKeymap.argtypes=[ctypes.c_void_p,ctypes.c_void_p]; x.XQueryKeymap(d,b); print(bool(b.raw[k//8] & (1 << (k%8))))"
            return subprocess.check_output(['docker','exec',container,'python3','-c',code],text=True).strip()=='True'
        wait(shift,'Human Shift reached actual X display')
        released=operate('releaseControl',controllerId=controller)
        assert released['control']['owner']['kind']=='idle'
        assert not shift(),'Held Shift survived handoff ACK'
        # A spectator survives control-epoch changes and continues receiving real pixels.
        b.frame()
        next_handoff=operate('takeControl')
        next_control=Viewer(url,grant(viewer_id,next_handoff,True));viewers.append(next_control)
        wait(lambda:state()['control']['owner']['kind']=='human','Second controller handshake')
        a.close();control.close()
        assert state()['control']['owner']['kind']=='human','Delayed old close suspended a newer controller'
        next_control.close()
        wait(lambda:state()['control']['owner']['kind']=='suspended','Controller disconnect keeps automation paused')
        b.frame()
        operate('resume')
        assert state()['control']['owner']['kind']=='idle'
        # Read-only protocol validation happens at the Worker, even if the UI is bypassed.
        b.key(0xff0d,True)
        try:
            while True:b.socket.recv(timeout=5)
        except ConnectionClosed:pass
        assert state()['control']['owner']['kind']=='idle','Read-only close paused Agent'
        (report/'media-acceptance.json').write_text(json.dumps({'status':'passed','rawFramebufferBytes':bytes_observed,
            'crossOriginRejected':True,'wrongTokenRejected':True,'otherAccountRejected':True,'ticketReplayRejected':True,'twoViewers':True,'thirdViewerRejected':True,'controlEpochRenewed':True,
            'rejectedReplacementPreservesController':True,'heldShiftReleasedBeforeAck':True,'oldCloseFenced':True,'controllerDisconnectSuspended':True,'spectatorRemainedReadOnly':True}))
        print('PASS real public WSS / private-CA outbound DesktopMedia / two RFB viewers / controller fence and held-key cleanup',flush=True)
    finally:
        for viewer in viewers:viewer.close()


def pressure_and_expiry(url,node,http,data,wait,report):
    def state():return data(http(url+'/v1/browser',{'nodeId':node,'operation':{'kind':'read'}}))
    def grant(viewer):
        snapshot=state()
        return data(http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':viewer,'generation':snapshot['control']['generation'],'inputMode':'readOnly','controllerId':None,'inputEpoch':None}))
    stale=http(url+'/v1/desktop/open',{'nodeId':node,'targetId':'managed','viewerId':str(uuid.uuid4()),'generation':str(uuid.uuid4()),'inputMode':'readOnly','controllerId':None,'inputEpoch':None})
    assert stale['status']=='error','Stale generation was admitted'
    oversized=Viewer(url,wait(lambda:grant(str(uuid.uuid4())),'Viewer slot after cleanup'))
    oversized.socket.send(b'\0'*65537)
    try:
        while True:oversized.socket.recv(timeout=5)
    except ConnectionClosed:pass
    oversized.close()
    blocked=Viewer(url,wait(lambda:grant(str(uuid.uuid4())),'Pressure viewer slot'),max_queue=1)
    request=struct.pack('>BBHHHH',3,0,0,0,blocked.width,blocked.height)
    started=time.monotonic()
    sent=0
    for _ in range(20):
        try:blocked.socket.send(request);sent+=1
        except ConnectionClosed:break
        # Permit each non-incremental framebuffer request to become a distinct frame.
        time.sleep(.4)
    assert state()['control']['owner']['kind']=='idle','Slow spectator blocked the control path'
    time.sleep(4)
    closed=False;received=0
    try:
        while received<32*1024*1024:
            received+=len(blocked.socket.recv(timeout=3))
    except ConnectionClosed:closed=True
    except TimeoutError:
        (report/'media-pressure-diagnostic.json').write_text(json.dumps({'framebufferRequests':sent,'drainedBytes':received,'socketState':str(blocked.socket.state)}))
    blocked.close()
    assert closed,'Backpressure did not terminate the stalled viewer'
    assert state()['control']['owner']['kind']=='idle'
    expired=wait(lambda:grant(str(uuid.uuid4())),'Expired ticket viewer slot')
    time.sleep(31)
    with connect(url.replace('http','ws',1)+'/v1/desktop/stream',origin=url,proxy=None,open_timeout=5) as socket:
        socket.send(json.dumps({'ticket':expired['ticket'],'accessToken':'browser-contract-alice'}))
        try:socket.recv(timeout=5);raise AssertionError('Expired grant sent data')
        except ConnectionClosed:pass
    (report/'media-pressure-acceptance.json').write_text(json.dumps({'status':'passed','staleGenerationRejected':True,'oversizedFrameRejected':True,'expiredTicketRejected':True,'stalledViewerTerminated':True,'controlPathResponsive':True,'framebufferRequests':sent,'drainedBytes':received,'pressureElapsedSeconds':time.monotonic()-started-31},indent=2))
    print('PASS expired/stale grants, oversized frames and bounded stalled-viewer backpressure',flush=True)
