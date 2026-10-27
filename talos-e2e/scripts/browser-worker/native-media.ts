import assert from 'node:assert/strict';
import path from 'node:path';
import {appendFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {execFileSync} from 'node:child_process';
import {app,BrowserWindow} from 'electron';
import {DesktopAppServerController} from '../../../talos-app/desktop/src/app-server-controller';
import {handleDesktopServerRequest} from '../../../talos-app/desktop/src/desktop-server-requests';
import {registerDesktopIpcHandlers} from '../../../talos-app/desktop/src/desktop-ipc';
import {desktopMediaHost} from '../../../talos-app/desktop/src/desktop-media';
import {desktopBrowserHost} from '../../../talos-app/desktop/src/browser-host';
const root=process.env.TALOS_RESOURCE_DIR!;
const output=process.env.TALOS_SCENARIO_REPORT_DIR??path.join(root,'output/browser-worker');
mkdirSync(output,{recursive:true});
app.on('window-all-closed',()=>{});
app.setPath('userData',path.join(process.env.TALOS_HOME!,'electron-profile'));
async function waitFor(probe:()=>Promise<boolean>,message:string){
 const deadline=Date.now()+10000;
 while(Date.now()<deadline){if(await probe())return;await new Promise(resolve=>setTimeout(resolve,100));}
 assert.fail(message);
}
async function run(){await app.whenReady();
 const controller=new DesktopAppServerController({command:process.env.TALOS_APP_SERVER_BIN,env:{...process.env,TALOS_DESKTOP_HOSTNAME:'native-media-acceptance'},desktopHost:true,clientKind:'desktop',onRuntimeState:state=>console.log('RUNTIME',state),onServerRequest:async request=>{
  const started=Date.now();
  try{return await handleDesktopServerRequest(request);}
  finally{if(request.method==='client/desktop/frame')appendFileSync(path.join(output,'native-media-frames.jsonl'),JSON.stringify({at:started,elapsedMs:Date.now()-started,bytes:(request.params as {dataBase64?:string}).dataBase64?.length??0})+'\n');}
 }});
 let window:BrowserWindow|undefined;
 try{
  await controller.connect();registerDesktopIpcHandlers(controller);
  window=new BrowserWindow({width:1200,height:760,show:true,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false,preload:path.join(root,'talos-app/desktop/dist/preload.cjs')}});
  await window.loadFile(path.join(import.meta.dirname,'../viewer-dist/index.html'),{query:{node:process.env.TALOS_E2E_REMOTE_NODE!}});
  const deadline=Date.now()+45000;let result:unknown;
  while(Date.now()<deadline){const error=await window.webContents.executeJavaScript('window.nativeMediaError');if(error)throw new Error(JSON.stringify(error));result=await window.webContents.executeJavaScript('window.nativeMediaResult');if(result)break;await new Promise(r=>setTimeout(r,100));}
  assert(result,'Actual noVNC did not connect through Desktop IPC');
  const view=window.webContents;
  const pixels=async()=>await view.executeJavaScript(`(()=>{const c=document.querySelector('canvas');if(!c||!c.width||!c.height)return 0;const x=c.getContext('2d'),samples=new Set();for(let i=0;i<64;i++){const color=x.getImageData(Math.floor(c.width*(i%8)/8),Math.floor(c.height*Math.floor(i/8)/8),1,1).data;samples.add(Array.from(color).join(','));}return samples.size;})()`)>3;
  await waitFor(pixels,'Actual RFB pixels did not render');
  assert.equal(await window.webContents.executeJavaScript("document.querySelector('#status').textContent"),'Docker Worker connected');
  assert.equal(window.webContents.debugger.isAttached(),false);
  const screenshot=await window.webContents.capturePage();writeFileSync(path.join(output,'native-docker-viewer.png'),screenshot.toPNG());
  writeFileSync(path.join(output,'native-docker-viewer.json'),JSON.stringify(result,null,2));
  const owner=()=>view.executeJavaScript('window.nativeMedia.read().then(snapshot=>snapshot.control.owner.kind)');
  const shift=()=>execFileSync('python3',['-c','import sys; sys.path.insert(0,sys.argv[1]); from media import shift_pressed; print(shift_pressed(sys.argv[2]))',path.join(root,'talos-e2e/scripts/browser-worker'),process.env.TALOS_E2E_WORKER_CONTAINER!],{encoding:'utf8',timeout:10000}).trim()==='True';
  await view.executeJavaScript('window.nativeMedia.takeControl()');
  await waitFor(async()=>await owner()==='human','Desktop IPC did not acquire control');
  await view.executeJavaScript('window.nativeMedia.holdShift()');
  await waitFor(async()=>shift(),'Desktop IPC key did not reach the actual X display');
  await waitFor(pixels,'Desktop IPC control did not render pixels');
  writeFileSync(path.join(output,'native-docker-control.png'),(await view.capturePage()).toPNG());
  await view.executeJavaScript('window.nativeMedia.release()');
  assert.equal(await owner(),'idle');assert.equal(shift(),false,'Held key survived Desktop IPC release');
  await view.executeJavaScript('window.nativeMedia.takeControl()');
  await waitFor(async()=>await owner()==='human','Second Desktop IPC control failed');
  await view.executeJavaScript('window.nativeMedia.holdShift()');
  await waitFor(async()=>shift(),'Second Desktop IPC key did not reach X');
  await view.executeJavaScript('window.nativeMedia.disconnect()');
  await waitFor(async()=>await owner()==='suspended','Desktop IPC disconnect did not pause automation');
  await waitFor(async()=>!shift(),'Desktop IPC disconnect did not clear held input');
  await view.executeJavaScript('window.nativeMedia.resume()');
  assert.equal(await owner(),'idle');
  await waitFor(pixels,'Desktop IPC resumed without rendering pixels');
  writeFileSync(path.join(output,'native-docker-recovered.png'),(await view.capturePage()).toPNG());
  writeFileSync(path.join(output,'native-docker-control.json'),JSON.stringify({status:'passed',actualX11Input:true,releaseClearedHeldKey:true,disconnectSuspended:true,disconnectClearedHeldKey:true,resumed:true,resumedPixels:true},null,2));
  console.log('NATIVE_DOCKER_MEDIA_ACCEPTANCE_PASS');
 }catch(error){
  if(window){
   writeFileSync(path.join(output,'native-docker-viewer-failure.json'),JSON.stringify(await window.webContents.executeJavaScript(`({status:document.querySelector('#status')?.textContent,result:window.nativeMediaResult,diagnostics:window.nativeMediaDiagnostics})`),null,2));
   writeFileSync(path.join(output,'native-docker-viewer-failure.png'),(await window.webContents.capturePage()).toPNG());
  }
  throw error;
 }finally{desktopMediaHost.reset();window?.destroy();await controller.close();await desktopBrowserHost.reset();app.quit();}
}
void run().catch(error=>{console.error(error);app.exit(1);});
