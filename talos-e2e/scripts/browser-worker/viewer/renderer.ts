import RFB from '@novnc/novnc/lib/rfb';
import {createElectronAppServerClient} from '../../../../talos-app/ts-sdk/packages/app-server/src/desktop';
const bridge=(window as unknown as {talos:import('@talos/sdk-app-server/desktop').DesktopRendererBridge}).talos;
const options={actorCredentials:{credential:{type:'existing_session_token' as const,token:'browser-contract-alice'}}};
const nodeId=new URL(location.href).searchParams.get('node')!;
const status=document.querySelector('#status')!;
const diagnostics:{frames:{at:number;bytes:number;elapsedMs:number}[];errors:string[];disconnect?:unknown}={frames:[],errors:[]};
(window as unknown as {nativeMediaDiagnostics:unknown}).nativeMediaDiagnostics=diagnostics;
window.addEventListener('error',event=>diagnostics.errors.push(event.message));
window.addEventListener('unhandledrejection',event=>diagnostics.errors.push(String(event.reason)));
(async()=>{
 const actor=await bridge.invoke<import('@talos/sdk-core').AppProtocol.UserCurrentResponse>('user/current',{},options);
 if(actor.status==='error')throw actor.error;
 const response=await bridge.invoke<import('@talos/sdk-core').AppProtocol.BrowserSnapshot>('browser/operate',{nodeId,operation:{kind:'read'}},options);
 if(response.status==='error')throw response.error;
 const snapshot=response.data;
 if(snapshot.display.kind!=='dockerDesktop')throw new Error('Remote node has no Docker desktop.');
 const admitted=await bridge.invoke<import('@talos/sdk-core').AppProtocol.DesktopViewGrant>('desktop/open',{nodeId,targetId:'managed',viewerId:crypto.randomUUID(),generation:snapshot.control.generation,inputMode:'readOnly',controllerId:null,inputEpoch:null},options);
 if(admitted.status==='error')throw admitted.error;
 const observedBridge={...bridge,onDesktopFrame:(handler:Parameters<NonNullable<typeof bridge.onDesktopFrame>>[0])=>bridge.onDesktopFrame!(frame=>{
  const at=Date.now();try{handler(frame);}finally{diagnostics.frames.push({at,bytes:frame.dataBase64?.length??0,elapsedMs:Date.now()-at});}
 })};
 const client=createElectronAppServerClient(observedBridge,{getActorCredentials:()=>options.actorCredentials});
 const socket=client.desktop.connect(admitted.data);
 socket.addEventListener('error',event=>diagnostics.errors.push(String((event as ErrorEvent).error)));
 const remote=new RFB(document.querySelector('#desktop') as HTMLElement,socket,{shared:true});remote.viewOnly=true;remote.scaleViewport=true;
 remote.addEventListener('connect',()=>{status.textContent='Docker Worker connected';(window as unknown as {nativeMediaResult:unknown}).nativeMediaResult={connected:true,streamId:admitted.data.streamId};});
 remote.addEventListener('disconnect',event=>{diagnostics.disconnect=(event as CustomEvent).detail;status.textContent='Docker Worker disconnected';});
})().catch(error=>{status.textContent=String(error);(window as unknown as {nativeMediaError:unknown}).nativeMediaError=error;});
