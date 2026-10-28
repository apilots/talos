import RFB from '@novnc/novnc/lib/rfb';
import type {BrowserSnapshot,DesktopViewGrant} from '@talos/sdk-core';
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
 const observedBridge={...bridge,onDesktopFrame:(handler:Parameters<NonNullable<typeof bridge.onDesktopFrame>>[0])=>bridge.onDesktopFrame!(frame=>{
  const at=Date.now();try{handler(frame);}finally{diagnostics.frames.push({at,bytes:frame.dataBase64?.length??0,elapsedMs:Date.now()-at});}
 })};
 const client=createElectronAppServerClient(observedBridge,{getActorCredentials:()=>options.actorCredentials});
 const read=()=>client.browser({nodeId,operation:{kind:'read'}});
 const viewerId=crypto.randomUUID();
 let remote:RFB|undefined,grant:DesktopViewGrant;
 async function mount(snapshot:BrowserSnapshot,control=false){
  if(snapshot.display.kind!=='dockerDesktop')throw new Error('Remote node has no Docker desktop.');
  const owner=snapshot.control.owner;
  if(control&&owner.kind!=='handingOver')throw new Error('Desktop control was not offered.');
  grant=await client.desktop.open({nodeId,targetId:'managed',viewerId,generation:snapshot.control.generation,
   inputMode:control?'control':'readOnly',controllerId:control&&owner.kind==='handingOver'?owner.controllerId:null,inputEpoch:control?snapshot.control.epoch:null});
  remote?.disconnect();
  const socket=client.desktop.connect(grant);
  socket.addEventListener('error',event=>diagnostics.errors.push(String((event as ErrorEvent).error)));
  const rfb=new RFB(document.querySelector('#desktop') as HTMLElement,socket,{shared:true});remote=rfb;
  rfb.viewOnly=true;rfb.scaleViewport=true;
  await new Promise<void>((resolve,reject)=>{
   rfb.addEventListener('connect',()=>{rfb.viewOnly=!control;status.textContent=control?'Docker Worker in control':'Docker Worker connected';resolve();});
   rfb.addEventListener('disconnect',event=>{if(remote===rfb){diagnostics.disconnect=(event as CustomEvent).detail;status.textContent='Docker Worker disconnected';}reject(new Error('Desktop IPC viewer disconnected'));});
  });
 }
 Object.assign(window,{nativeMedia:{read,
  takeControl:async()=>{const snapshot=await read();await mount(await client.browser({nodeId,operation:{kind:'takeControl',expectedRevision:snapshot.control.revision}}),true);},
  release:async()=>{const snapshot=await read();await mount(await client.browser({nodeId,operation:{kind:'releaseControl',controllerId:grant.controllerId!,expectedRevision:snapshot.control.revision}}));},
  resume:async()=>{const snapshot=await read();await mount(await client.browser({nodeId,operation:{kind:'resume',expectedRevision:snapshot.control.revision}}));},
  holdShift:()=>remote!.sendKey(0xffe1,'ShiftLeft',true),
  disconnect:()=>remote?.disconnect(),
 }});
 await mount(await read());
 (window as unknown as {nativeMediaResult:unknown}).nativeMediaResult={connected:true,streamId:grant!.streamId};
})().catch(error=>{status.textContent=String(error);(window as unknown as {nativeMediaError:unknown}).nativeMediaError=error;});
