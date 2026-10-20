import assert from 'node:assert/strict';
import {mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {app} from 'electron';
import {DesktopAppServerController} from '../../../talos-app/desktop/src/app-server-controller';
import {handleDesktopServerRequest} from '../../../talos-app/desktop/src/desktop-server-requests';
import {desktopBrowserHost} from '../../../talos-app/desktop/src/browser-host';
import type {AppProtocol as P} from '@talos/sdk-core';
const root=process.env.TALOS_RESOURCE_DIR!;
const output=process.env.TALOS_SCENARIO_REPORT_DIR??path.join(root,'output/browser-worker');
mkdirSync(output,{recursive:true});
app.on('window-all-closed',()=>{});
app.setPath('userData',path.join(process.env.TALOS_HOME!,'electron-profile'));
async function run(){console.log("NATIVE_MCP_START");await app.whenReady();console.log("NATIVE_MCP_READY");
 const events:P.TimelineEvent[]=[];
 const controller=new DesktopAppServerController({command:process.env.TALOS_APP_SERVER_BIN,env:{...process.env,TALOS_DESKTOP_HOSTNAME:'native-browser-acceptance'},desktopHost:true,clientKind:"desktop",onRuntimeState:state=>console.log("RUNTIME",state),onEvent:event=>events.push(event),onServerRequest:request=>handleDesktopServerRequest(request)});
 try {
  console.log("CONNECTING");await controller.connect();console.log("CONNECTED");
  console.log('READ_BROWSER');const state=await controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation:{kind:'read'}});
  assert.equal(state.host,'desktop');assert.equal(state.display.kind,'unavailable');assert.equal(state.selectedTargetId,'managed');
  const upload=await controller.invoke<P.FileUploadResponse>('file/upload',{request:{filename:'browser-upload.png',scope:{type:'personal'},tags:[],metadata:{}},dataBase64:'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a5n8AAAAASUVORK5CYII='});
  const sessionId=randomUUID();

  await controller.invoke('turn/start',{nodeId:'embedded',sessionId,input:{content:[{type:'text',text:'Run the deterministic browser acceptance. BROWSER_UPLOAD_FILE_ID='+upload.file.fileId},{type:'file',fileId:upload.file.fileId}]},modelSelection:{modelRef:{provider:'openai',id:'browser-fixture'},reasoningEffort:null},projectId:null,currentDirectory:null,userPermissionMode:'fullAccess',clientRequestId:randomUUID()});
  await controller.invoke('event/subscribe',{sessionId,turnId:null,sinceEventId:null,replayLimit:50,closeWhenIdle:false,closeOnTerminal:false});
  const deadline=Date.now()+90000;let history:unknown;
  while(Date.now()<deadline){history=await controller.invoke('session/history',{sessionId,limit:100,cursor:null});if(JSON.stringify(history).includes('BROWSER_WORKER_ACCEPTANCE_OK'))break;await new Promise(r=>setTimeout(r,200));}
  writeFileSync(path.join(output,'native-mcp-events.json'),JSON.stringify(events,null,2));writeFileSync(path.join(output,'native-mcp-history.json'),JSON.stringify(history,null,2));
  assert.match(JSON.stringify(history),/BROWSER_WORKER_ACCEPTANCE_OK/);
  assert.match(JSON.stringify(history),/Hello Worker/);
  assert.match(JSON.stringify(history),/browser_screenshot/);
  const findFile=(value:unknown):string|undefined=>{
    if(typeof value==='string'&&value.startsWith('{')){try{return findFile(JSON.parse(value));}catch{return undefined;}}
    if(typeof value==='object'&&value!==null){const file=value as Record<string,unknown>;if(file.originalFilename==='report.csv'&&typeof file.fileId==='string')return file.fileId;for(const child of Object.values(file)){const found=findFile(child);if(found)return found;}}
    return undefined;
  };
  const downloaded=findFile(history);assert(downloaded);
  const content=await controller.invoke<P.FileContentResponse>('file/content',{fileId:downloaded});assert.equal(Buffer.from(content.dataBase64,'base64').toString(),'name,value\nworker,1\n');
  let observed=await controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation:{kind:'read'}});
  for(let attempt=0;observed.control.owner.kind==='agent'&&attempt<50;attempt++) {
    await new Promise(resolve=>setTimeout(resolve,50));
    observed=await controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation:{kind:'read'}});
  }
  const paused=await controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation:{kind:'pause',expectedRevision:observed.control.revision}});
  assert.equal(paused.control.owner.kind,'suspended');
  const resumed=await controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation:{kind:'resume',expectedRevision:paused.control.revision}});
  assert.equal(resumed.control.owner.kind,'idle');
  const resumedSession=randomUUID();
  await controller.invoke('turn/start',{nodeId:'embedded',sessionId:resumedSession,input:{content:[{type:'text',text:'Repeat after native pause/resume. BROWSER_UPLOAD_FILE_ID='+upload.file.fileId},{type:'file',fileId:upload.file.fileId}]},modelSelection:{modelRef:{provider:'openai',id:'browser-fixture'},reasoningEffort:null},projectId:null,currentDirectory:null,userPermissionMode:'fullAccess',clientRequestId:randomUUID()});
  let resumedHistory:unknown;const resumedDeadline=Date.now()+90000;
  while(Date.now()<resumedDeadline){resumedHistory=await controller.invoke('session/history',{sessionId:resumedSession,limit:100,cursor:null});if(JSON.stringify(resumedHistory).includes('BROWSER_WORKER_ACCEPTANCE_OK'))break;await new Promise(resolve=>setTimeout(resolve,200));}
  assert.match(JSON.stringify(resumedHistory),/BROWSER_WORKER_ACCEPTANCE_OK/);assert.match(JSON.stringify(resumedHistory),/Hello Worker/);
  writeFileSync(path.join(output,'native-resumed-history.json'),JSON.stringify(resumedHistory,null,2));
  writeFileSync(path.join(output,'native-mcp-acceptance.json'),JSON.stringify({status:'passed',sessionId,resumedSession,pauseResumeAndFreshObservation:true,downloadedFileId:downloaded},null,2));
  console.log('NATIVE_MCP_ACCEPTANCE_PASS');
 }finally{await controller.close();await desktopBrowserHost.reset();app.quit();}
}
void run().catch(error=>{console.error(error);app.exit(1);});
