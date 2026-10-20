import {describe,expect,it,vi} from 'vitest';
import type {AppProtocol} from '@talos/sdk-core';
import type {DesktopRendererBridge} from '@talos/sdk-app-server/desktop';
import {createNativeDesktopStream} from './desktop-stream';
import {createElectronAppServerClient} from './desktop';
import {TalosApiError} from '@talos/sdk-core';
const grant={streamId:'stream',ticket:'one-use-ticket'} as AppProtocol.DesktopViewGrant;
function fixture(){let receive:((frame:AppProtocol.DesktopMediaFrame)=>void)|undefined;const invoke=vi.fn(async()=>({status:'success',data:null}));const remove=vi.fn();const bridge={invoke,onDesktopFrame:(handler:typeof receive)=>{receive=handler;return remove;}} as unknown as DesktopRendererBridge;return{invoke,bridge,remove,receive:(frame:AppProtocol.DesktopMediaFrame)=>receive?.(frame)};}
describe('Desktop noVNC protected raw channel',()=>{
 it('binds admission to the current account and routes only its immutable stream',async()=>{
  const f=fixture(),socket=createNativeDesktopStream(f.bridge,grant,{credential:{type:'existing_session_token',token:'account-token'}}),frames=vi.fn();socket.onmessage=frames;
  await Promise.resolve();await Promise.resolve();
  expect(f.invoke).toHaveBeenCalledWith('desktop/streamOpen',{streamId:'stream',ticket:'one-use-ticket'},{actorCredentials:{credential:{type:'existing_session_token',token:'account-token'}}});
  f.receive({streamId:'other',dataBase64:'UkZC'});expect(frames).not.toHaveBeenCalled();
  f.receive({streamId:'stream',dataBase64:'UkZC'});expect(new Uint8Array(frames.mock.calls[0][0].data)).toEqual(new Uint8Array([82,70,66]));
  socket.send(new Uint8Array([1,2,3]));await Promise.resolve();await Promise.resolve();
  expect(f.invoke).toHaveBeenCalledWith('desktop/streamSend',{streamId:'stream',dataBase64:'AQID'},expect.anything());
  socket.close();expect(f.remove).toHaveBeenCalledOnce();expect(socket.readyState).toBe(3);
 });
 it('closes instead of buffering an oversized input or accepting oversized frames',async()=>{
  const f=fixture(),socket=createNativeDesktopStream(f.bridge,grant,{credential:{type:'existing_session_token',token:'account-token'}});
  await Promise.resolve();await Promise.resolve();
  socket.send(new Uint8Array(4*1024*1024+1));expect(socket.readyState).toBe(3);
  const second=createNativeDesktopStream(f.bridge,grant,{credential:{type:'existing_session_token',token:'account-token'}});await Promise.resolve();await Promise.resolve();
  f.receive({streamId:'stream',dataBase64:'A'.repeat(88*1024+1)});expect(second.readyState).toBe(3);
 });
});

it('exposes the protected media lifecycle through the public Electron SDK and preserves typed failure',async()=>{
 const f=fixture();const client=createElectronAppServerClient(f.bridge,{getActorCredentials:()=>({credential:{type:'existing_session_token',token:'sdk-token'}})});
 const socket=client.desktop.connect(grant),failure=vi.fn();socket.onerror=failure;
 await Promise.resolve();await Promise.resolve();f.receive({streamId:'stream',dataBase64:'invalid%base64'});
 expect(failure).toHaveBeenCalledOnce();expect(socket.readyState).toBe(3);
 const second=fixture();second.invoke.mockResolvedValueOnce({status:'error',error:{code:'permissionDenied',message:'Desktop stream denied.',localization:null}} as never);
 const rejected=createElectronAppServerClient(second.bridge).desktop.connect(grant),error=vi.fn();rejected.onerror=error;
 await Promise.resolve();await Promise.resolve();await Promise.resolve();
 expect(error.mock.calls[0][0].error).toBeInstanceOf(TalosApiError);expect(rejected.readyState).toBe(3);
});
