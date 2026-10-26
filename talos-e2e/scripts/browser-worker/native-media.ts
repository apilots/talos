import assert from 'node:assert/strict';
import path from 'node:path';
import {appendFileSync,mkdirSync,writeFileSync} from 'node:fs';
import {app} from 'electron';
import {createDesktopWindow} from '../../../talos-app/desktop/src/desktop-window';
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
async function run(){await app.whenReady();
 const controller=new DesktopAppServerController({command:process.env.TALOS_APP_SERVER_BIN,env:{...process.env,TALOS_DESKTOP_HOSTNAME:'native-media-acceptance'},desktopHost:true,clientKind:'desktop',onRuntimeState:state=>console.log('RUNTIME',state),onServerRequest:async request=>{
  const started=Date.now();
  try{return await handleDesktopServerRequest(request);}
  finally{if(request.method==='client/desktop/frame')appendFileSync(path.join(output,'native-media-frames.jsonl'),JSON.stringify({at:started,elapsedMs:Date.now()-started,bytes:(request.params as {dataBase64?:string}).dataBase64?.length??0})+'\n');}
 }});
 let product:ReturnType<typeof createDesktopWindow>|undefined;
 try{
  await controller.connect();registerDesktopIpcHandlers(controller);
  product=createDesktopWindow({width:1200,height:760,show:true},{sandbox:true,contextIsolation:true,nodeIntegration:false,preload:path.join(root,'talos-app/desktop/dist/preload.cjs')});
  await product.view.webContents.loadFile(path.join(import.meta.dirname,'../viewer-dist/index.html'),{query:{node:process.env.TALOS_E2E_REMOTE_NODE!}});
  const deadline=Date.now()+45000;let result:unknown;
  while(Date.now()<deadline){const error=await product.view.webContents.executeJavaScript('window.nativeMediaError');if(error)throw new Error(JSON.stringify(error));result=await product.view.webContents.executeJavaScript('window.nativeMediaResult');if(result)break;await new Promise(r=>setTimeout(r,100));}
  assert(result,'Actual noVNC did not connect through Desktop IPC');
  let pixels=0;
  for(let attempt=0;attempt<100;attempt++){
    pixels=await product.view.webContents.executeJavaScript(`(()=>{const c=document.querySelector('canvas');if(!c||!c.width||!c.height)return 0;const x=c.getContext('2d'),samples=new Set();for(let i=0;i<64;i++){const color=x.getImageData(Math.floor(c.width*(i%8)/8),Math.floor(c.height*Math.floor(i/8)/8),1,1).data;samples.add(Array.from(color).join(','));}return samples.size;})()`);
    if(pixels>3)break;await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert(pixels>3,'Actual RFB pixels did not render');
  assert.equal(await product.view.webContents.executeJavaScript("document.querySelector('#status').textContent"),'Docker Worker connected');
  assert.equal(product.view.webContents.debugger.isAttached(),false);
  const screenshot=await product.view.webContents.capturePage();writeFileSync(path.join(output,'native-docker-viewer.png'),screenshot.toPNG());
  writeFileSync(path.join(output,'native-docker-viewer.json'),JSON.stringify(result,null,2));
  console.log('NATIVE_DOCKER_MEDIA_ACCEPTANCE_PASS');
 }catch(error){
  if(product){
   writeFileSync(path.join(output,'native-docker-viewer-failure.json'),JSON.stringify(await product.view.webContents.executeJavaScript(`({status:document.querySelector('#status')?.textContent,result:window.nativeMediaResult,diagnostics:window.nativeMediaDiagnostics})`),null,2));
   writeFileSync(path.join(output,'native-docker-viewer-failure.png'),(await product.view.webContents.capturePage()).toPNG());
  }
  throw error;
 }finally{desktopMediaHost.reset();product?.window.destroy();await controller.close();await desktopBrowserHost.reset();app.quit();}
}
void run().catch(error=>{console.error(error);app.exit(1);});
