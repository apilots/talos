import assert from 'node:assert/strict';
import {mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import {randomUUID} from 'node:crypto';
import {app,webContents} from 'electron';
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
  const operate=(operation:P.BrowserOperation)=>controller.invoke<P.BrowserSnapshot>('browser/operate',{nodeId:'embedded',operation});
  const findFile=(value:unknown):string|undefined=>{
    if(typeof value==='string'&&value.startsWith('{')){try{return findFile(JSON.parse(value));}catch{return undefined;}}
    if(typeof value==='object'&&value!==null){const file=value as Record<string,unknown>;if(file.originalFilename==='report.csv'&&typeof file.fileId==='string')return file.fileId;for(const child of Object.values(file)){const found=findFile(child);if(found)return found;}}
    return undefined;
  };
  async function idleSnapshot(){
    let observed=await operate({kind:'read'});
    for(let attempt=0;observed.control.owner.kind==='agent'&&attempt<50;attempt++){
      await new Promise(resolve=>setTimeout(resolve,50));
      observed=await operate({kind:'read'});
    }
    assert.equal(observed.control.owner.kind,'idle');
    return observed;
  }
  async function task(mode:P.BrowserTargetKind,historyFile:string){
    const before=await idleSnapshot();assert.equal(before.selectedTargetId,mode);
    const sessionId=randomUUID();
    await controller.invoke('turn/start',{nodeId:'embedded',sessionId,input:{content:[{type:'text',text:'Run the deterministic browser acceptance in '+mode+'. BROWSER_UPLOAD_FILE_ID='+upload.file.fileId},{type:'file',fileId:upload.file.fileId}]},modelSelection:{modelRef:{provider:'openai',id:'browser-fixture'},reasoningEffort:null},projectId:null,currentDirectory:null,userPermissionMode:'fullAccess',clientRequestId:randomUUID()});
    await controller.invoke('event/subscribe',{sessionId,turnId:null,sinceEventId:null,replayLimit:50,closeWhenIdle:false,closeOnTerminal:false});
    const deadline=Date.now()+90000;let history:P.SessionHistoryResponse|undefined;
    while(Date.now()<deadline){history=await controller.invoke<P.SessionHistoryResponse>('session/history',{sessionId,limit:100,cursor:null});if(JSON.stringify(history).includes('BROWSER_WORKER_ACCEPTANCE_OK'))break;await new Promise(resolve=>setTimeout(resolve,200));}
    writeFileSync(path.join(output,historyFile),JSON.stringify(history,null,2));
    assert(history);assert.match(JSON.stringify(history),/BROWSER_WORKER_ACCEPTANCE_OK/);
    assert.match(JSON.stringify(history),/Hello Worker/);
    const tools=history.data.flatMap(turn=>turn.result.result.messages).flatMap(message=>message.blocks).flatMap(block=>block.content.type==='toolResult'?[block.content.data]:[]);
    for(const tool of ['browser_navigate','browser_state','browser_fill','browser_click','browser_screenshot','browser_download'])assert(tools.some(result=>result.toolName.endsWith(tool)&&!result.isError),mode+' missing successful '+tool);
    const uploads=tools.filter(result=>result.toolName.endsWith('browser_upload'));
    assert.equal(uploads.length,2);assert.equal(uploads.filter(result=>result.isError).length,1);
    assert.match(JSON.stringify(uploads.find(result=>!result.isError)),/browser-upload.png/);
    const screenshot=tools.find(result=>result.toolName.endsWith('browser_screenshot'));assert(screenshot);
    const image=screenshot.content.find(item=>item.type==='image');assert(image&&image.type==='image');
    const imageContent=await controller.invoke<P.FileContentResponse>('file/content',{fileId:image.fileId});
    const png=Buffer.from(imageContent.dataBase64,'base64');assert.equal(png.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
    const pageScreenshot='native-mcp-'+mode+'.png';writeFileSync(path.join(output,pageScreenshot),png);
    const page=screenshot.content.find(item=>item.type==='json');assert(page&&page.type==='json');
    const targetId=(page.value as {targetId?:unknown}).targetId;assert.equal(typeof targetId,'string');
    if(mode==='connected'){
      const targets=await (await fetch('http://127.0.0.1:9222/json/list')).json() as {id:string}[];
      assert(targets.some(target=>target.id===targetId),'MCP must observe the owned prepared Chrome target');
    }
    const downloaded=findFile(tools.find(result=>result.toolName.endsWith('browser_download')));assert(downloaded);
    const content=await controller.invoke<P.FileContentResponse>('file/content',{fileId:downloaded});assert.equal(Buffer.from(content.dataBase64,'base64').toString(),'name,value\nworker,1\n');
    const after=await idleSnapshot();assert.equal(after.selectedTargetId,mode);
    writeFileSync(path.join(output,'native-mcp-'+mode+'-state.json'),JSON.stringify({before,after,targetId},null,2));
    console.log('NATIVE_MCP_MODE_PASS '+mode);
    return {status:'passed',mode,sessionId,downloadedFileId:downloaded,screenshotFileId:image.fileId,pageScreenshot,screenshotScope:'pageContent',targetId,attachmentRejection:true,authorizedUpload:true,downloadContent:true};
  }
  const managed=await task('managed','native-mcp-history.json');
  const observed=await idleSnapshot();
  const paused=await operate({kind:'pause',expectedRevision:observed.control.revision});
  assert.equal(paused.control.owner.kind,'suspended');
  const resumed=await operate({kind:'resume',expectedRevision:paused.control.revision});
  assert.equal(resumed.control.owner.kind,'idle');
  const selected=await operate({kind:'selectTarget',targetId:'connected',expectedRevision:resumed.control.revision});
  assert.equal(selected.selectedTargetId,'connected');
  const connected=await operate({kind:'connect',expectedRevision:selected.control.revision});
  assert.equal(connected.targets.find(target=>target.targetId==='connected')?.readiness.status,'ready');
  writeFileSync(path.join(output,'native-mcp-transitions.json'),JSON.stringify({paused,resumed,selected,connected},null,2));
  const prepared=await task('connected','native-resumed-history.json');
  assert.notEqual(managed.downloadedFileId,prepared.downloadedFileId);
  const finished=await idleSnapshot();
  await operate({kind:'disconnect',expectedRevision:finished.control.revision});
  assert.equal((await fetch('http://127.0.0.1:9222/json/version')).ok,true);
  writeFileSync(path.join(output,'native-mcp-events.json'),JSON.stringify(events,null,2));
  writeFileSync(path.join(output,'native-mcp-acceptance.json'),JSON.stringify({status:'passed',sessionId:managed.sessionId,resumedSession:prepared.sessionId,pauseResumeAndFreshObservation:true,selectedPreparedChromeAfterResume:true,ownedChromeSurvivesDisconnect:true,modes:{managed,connected:prepared}},null,2));
  console.log('NATIVE_MCP_ACCEPTANCE_PASS');
 }catch(error){
  // Preserve the actual page/input evidence before cleanup destroys its views.
  // This reads only the synthetic fixture page and does not change tool behavior.
  for(const page of webContents.getAllWebContents().filter(page=>page.getURL()==='http://127.0.0.1:18080/')){
   try{
    const state=await page.executeJavaScript(`({value:document.querySelector('input')?.value,text:document.body.innerText,active:document.activeElement?.tagName,events:window.fixtureInputs})`);
    writeFileSync(path.join(output,'native-mcp-failure-'+page.id+'.json'),JSON.stringify({state,focused:page.isFocused()},null,2));
    writeFileSync(path.join(output,'native-mcp-failure-'+page.id+'.png'),(await page.capturePage()).toPNG());
   }catch(diagnosticError){console.error('Native MCP failure capture failed',diagnosticError);}
  }
  throw error;
 }finally{await controller.close();await desktopBrowserHost.reset();app.quit();}
}
void run().catch(error=>{console.error(error);app.exit(1);});
