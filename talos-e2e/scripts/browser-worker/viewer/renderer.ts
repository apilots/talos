import RFB from '@novnc/novnc/lib/rfb';
import {createElectronAppServerClient} from '../../../../talos-app/ts-sdk/packages/app-server/src/desktop';
const bridge=(window as unknown as {talos:import('@talos/sdk-app-server/desktop').DesktopRendererBridge}).talos;
const options={actorCredentials:{credential:{type:'existing_session_token' as const,token:'browser-contract-alice'}}};
const nodeId=new URL(location.href).searchParams.get('node')!;
const status=document.querySelector('#status')!;
(async()=>{
 const response=await bridge.invoke<import('@talos/sdk-core').AppProtocol.BrowserSnapshot>('browser/operate',{nodeId,operation:{kind:'read'}},options);
 if(response.status==='error')throw response.error;
 const snapshot=response.data;
 if(snapshot.display.kind!=='dockerDesktop')throw new Error('Remote node has no Docker desktop.');
 const admitted=await bridge.invoke<import('@talos/sdk-core').AppProtocol.DesktopViewGrant>('desktop/open',{nodeId,targetId:'managed',viewerId:crypto.randomUUID(),generation:snapshot.control.generation,inputMode:'readOnly',controllerId:null,inputEpoch:null},options);
 if(admitted.status==='error')throw admitted.error;
 const client=createElectronAppServerClient(bridge,{getActorCredentials:()=>options.actorCredentials});
 const socket=client.desktop.connect(admitted.data);
 const remote=new RFB(document.querySelector('#desktop') as HTMLElement,socket,{shared:true});remote.viewOnly=true;remote.scaleViewport=true;
 remote.addEventListener('connect',()=>{status.textContent='Docker Worker connected';(window as unknown as {nativeMediaResult:unknown}).nativeMediaResult={connected:true,streamId:admitted.data.streamId};});
 remote.addEventListener('disconnect',()=>{status.textContent='Docker Worker disconnected';});
})().catch(error=>{status.textContent=String(error);(window as unknown as {nativeMediaError:unknown}).nativeMediaError=error;});
