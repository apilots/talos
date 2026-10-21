import assert from 'node:assert/strict';
import path from 'node:path';
import {mkdirSync,writeFileSync} from 'node:fs';
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
async function run(){await app.whenReady();
 const controller=new DesktopAppServerController({command:process.env.TALOS_APP_SERVER_BIN,env:{...process.env,TALOS_DESKTOP_HOSTNAME:'native-media-acceptance'},desktopHost:true,clientKind:'desktop',onRuntimeState:state=>console.log('RUNTIME',state),onServerRequest:request=>handleDesktopServerRequest(request)});
 let window:BrowserWindow|undefined;
 try{
  await controller.connect();registerDesktopIpcHandlers(controller);
  window=new BrowserWindow({width:1200,height:760,show:true,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false,preload:path.join(root,'talos-app/desktop/dist/preload.cjs')}});
  await window.loadFile(path.join(root,'.tmp/browser-worker/native/viewer-dist/index.html'),{query:{node:process.env.TALOS_E2E_REMOTE_NODE!}});
  const deadline=Date.now()+45000;let result:unknown;
  while(Date.now()<deadline){const error=await window.webContents.executeJavaScript('window.nativeMediaError');if(error)throw new Error(JSON.stringify(error));result=await window.webContents.executeJavaScript('window.nativeMediaResult');if(result)break;await new Promise(r=>setTimeout(r,100));}
  assert(result,'Actual noVNC did not connect through Desktop IPC');
  let pixels=0;
  for(let attempt=0;attempt<100;attempt++){
    pixels=await window.webContents.executeJavaScript(`(()=>{const c=document.querySelector('canvas');if(!c||!c.width||!c.height)return 0;const x=c.getContext('2d'),samples=new Set();for(let i=0;i<64;i++){const color=x.getImageData(Math.floor(c.width*(i%8)/8),Math.floor(c.height*Math.floor(i/8)/8),1,1).data;samples.add(Array.from(color).join(','));}return samples.size;})()`);
    if(pixels>3)break;await new Promise(resolve=>setTimeout(resolve,100));
  }
  assert(pixels>3,'Actual RFB pixels did not render');
  assert.equal(await window.webContents.executeJavaScript("document.querySelector('#status').textContent"),'Docker Worker connected');
  assert.equal(window.webContents.debugger.isAttached(),false);
  const screenshot=await window.webContents.capturePage();writeFileSync(path.join(output,'native-docker-viewer.png'),screenshot.toPNG());
  writeFileSync(path.join(output,'native-docker-viewer.json'),JSON.stringify(result,null,2));
  console.log('NATIVE_DOCKER_MEDIA_ACCEPTANCE_PASS');
 }finally{desktopMediaHost.reset();window?.destroy();await controller.close();await desktopBrowserHost.reset();app.quit();}
}
void run().catch(error=>{console.error(error);app.exit(1);});
