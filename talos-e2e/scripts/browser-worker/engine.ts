import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {appendFileSync,mkdirSync,writeFileSync} from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {app,BrowserWindow,webContents} from 'electron';
import type {AppProtocol} from '@talos/sdk-core';
import {DesktopBrowserHost} from '../../../talos-app/desktop/src/browser-host';
const root=path.resolve(import.meta.dirname,'../../../..');
const output=process.env.TALOS_SCENARIO_REPORT_DIR??path.join(root,'output/browser-worker');
mkdirSync(output,{recursive:true});
const tracePath=path.join(output,'native-engine-cdp.jsonl');
writeFileSync(tracePath,'');
function trace(event:Record<string,unknown>){appendFileSync(tracePath,JSON.stringify({at:Date.now(),...event})+'\n');}
// Observe real CDP commands and replies without logging URLs, arguments or page data.
const NativeSocket=globalThis.WebSocket;
class ObservedChromeSocket extends NativeSocket {
 private readonly commands=new Map<number,{method:string;started:number}>();
 constructor(url:string|URL,protocols?:string|string[]) {
  super(url,protocols);
  this.addEventListener('message',event=>{
   let message:{id?:number;method?:string;error?:unknown};try{message=JSON.parse(String(event.data));}catch{return;}
   if(message.method&&['Page.frameStartedLoading','Page.frameStoppedLoading','Page.frameNavigated','Page.frameStartedNavigating','Page.javascriptDialogOpening','Inspector.targetCrashed'].includes(message.method))trace({phase:'pageEvent',method:message.method});
   const pending=message.id===undefined?undefined:this.commands.get(message.id);
   if(pending){this.commands.delete(message.id!);trace({phase:'reply',id:message.id,method:pending.method,elapsedMs:Date.now()-pending.started,rejected:Boolean(message.error)});}
  });
  this.addEventListener('close',()=>trace({phase:'socketClosed',pending:[...this.commands.entries()].map(([id,value])=>({id,method:value.method,elapsedMs:Date.now()-value.started}))}));
 }
 send(data:Parameters<WebSocket['send']>[0]):void {
  if(typeof data==='string') {
   let command:{id?:number;method?:string};try{command=JSON.parse(data);}catch{command={};}
   if(typeof command.id==='number'&&typeof command.method==='string'){this.commands.set(command.id,{method:command.method,started:Date.now()});trace({phase:'send',id:command.id,method:command.method});}
  }
  super.send(data);
 }
}
globalThis.WebSocket=ObservedChromeSocket;
app.on('window-all-closed',()=>{});
app.setPath('userData',path.join(root,'.tmp/browser-worker/native/profile-'+randomUUID()));
async function run(){
console.log('NATIVE_START');
await app.whenReady();
console.log('NATIVE_READY');
const uuidFilename='12345678-1234-1234-1234-123456789abc';
let finishPendingReport:(()=>void)|undefined;
const fixture=createServer((req,res)=>{const pathname=new URL(req.url??'/', 'http://127.0.0.1').pathname;trace({phase:'httpRequest',pathname});res.once('finish',()=>trace({phase:'httpResponseFinished',pathname,status:res.statusCode}));if(req.url==='/uuid'){res.writeHead(200,{'content-type':'application/pdf','content-disposition':`attachment; filename="${uuidFilename}"`});res.end('%PDF-1.7 UUID download');return;}if(req.url==='/report.csv'||req.url==='/report.csv?part=2'){res.writeHead(200,{'content-type':'text/csv','content-disposition':'attachment; filename="report.csv"'});if(req.url.endsWith('?part=2')){res.write('name,value\nworker,');finishPendingReport=()=>{res.end('2\n');finishPendingReport=undefined;};}else res.end('name,value\nworker,1\n');return;}res.writeHead(200,{'content-type':'text/html'});res.end(`<!doctype html><title>Talos browser acceptance</title><input id="name" placeholder="Name"><button id="hello" onclick="document.querySelector('h1').textContent='Hello '+document.querySelector('#name').value">Hello</button><h1>Ready</h1><input id="upload" type="file" onchange="document.querySelector('h2').textContent=this.files[0].name"><h2></h2><a id="download" href="/report.csv">Download</a><a id="second-download" href="/report.csv?part=2">Second report</a><a id="uuid-download" href="/uuid">UUID download</a><script>window.events=[];for(const type of ["mousedown","mouseup","click","pointerdown"])document.addEventListener(type,event=>window.events.push({type,x:event.clientX,y:event.clientY,target:event.target.id}));</script>`);});
await new Promise<void>(resolve=>fixture.listen(0,'127.0.0.1',resolve));
const address=fixture.address();assert(address&&typeof address!=='string');
const host=new DesktopBrowserHost(),generation=randomUUID();
let targetId:string|null=null;let selectedTarget:AppProtocol.BrowserTargetKind='managed';
async function operation(operation:AppProtocol.BrowserHostOperation,userId='alice',target=selectedTarget) {
 const started=Date.now(),action=operation.kind==='execute'?operation.action.kind:operation.kind;
 trace({phase:'operationStart',target,action});
 try {const result=await host.execute(randomUUID(),{generation,epoch:1,userId,target,operation});trace({phase:'operationDone',target,action,elapsedMs:Date.now()-started});return result;}
 catch(error){trace({phase:'operationFailed',target,action,elapsedMs:Date.now()-started});console.error(`NATIVE_OPERATION_FAILED ${target} ${action}`);throw error;}
}
async function action(action:AppProtocol.BrowserAction,element:AppProtocol.BrowserHostElement|null=null,upload:AppProtocol.BrowserHostUpload|null=null){const response=await operation({kind:'execute',action,targetId,element,upload});assert.equal(response.kind,'observation');if(response.kind!=='observation')throw Error('observation');targetId=response.page.targetId;return response;}
async function downloadLifecycle(){
 const expected=new Map([['report.csv','name,value\nworker,1\n'],['report (1).csv','name,value\nworker,2\n'],[uuidFilename,'%PDF-1.7 UUID download']]);
 const links=['download','second-download','uuid-download'];
 let files:AppProtocol.BrowserHostFile[]=[];
 for(const [index,id] of links.entries()) {
  const state=await action({kind:'state'}),link=state.elements.find(element=>element.attributes.id===id);assert(link);
  await action({kind:'click',element:'scoped-reference'},link);
  if(id==='second-download') {
   for(let attempt=0;attempt<50&&!finishPendingReport;attempt++)await new Promise(resolve=>setTimeout(resolve,100));
   assert(finishPendingReport,'Second download did not start.');
   const pending=await action({kind:'download'});
   assert.deepEqual(pending.downloads,files,'Incomplete download must remain private.');
   finishPendingReport();
  }
  files=(await action({kind:'download'})).downloads;
  for(let attempt=0;attempt<50&&files.length<index+1;attempt++){await new Promise(resolve=>setTimeout(resolve,100));files=(await action({kind:'download'})).downloads;}
  assert.equal(files.length,index+1);
 }
 for(const file of files)assert.equal(Buffer.from(file.dataBase64,'base64').toString(),expected.get(file.filename));
 assert.deepEqual(files.map(file=>file.filename).sort(),[...expected.keys()].sort());
 assert.deepEqual((await action({kind:'download'})).downloads,files);
 await operation({kind:'acknowledgeDownloads',files:files.map(file=>({filename:file.filename,sha256:createHash('sha256').update(Buffer.from(file.dataBase64,'base64')).digest('hex')}))});
 assert.equal((await action({kind:'download'})).downloads.length,0);
 console.log(`DOWNLOAD_COLLISION_UUID_PASS ${selectedTarget} ${JSON.stringify(files.map(file=>file.filename))}`);
}
async function tabLifecycle(){
 const original=await action({kind:'state'}),first=original.page.targetId,oldElement=original.elements.find(e=>e.attributes.id==='name');assert(oldElement);
 const opened=await action({kind:'tab',action:'open',tabId:null,url:`http://127.0.0.1:${address.port}/second`});
 const second=opened.page.targetId;assert.notEqual(second,first);assert(opened.page.tabs.some(tab=>tab.tabId===first));
 await assert.rejects(action({kind:'fill',element:'old-tab-reference',text:'must not enter another tab'},oldElement));
 const third=(await action({kind:'tab',action:'open',tabId:null,url:`http://127.0.0.1:${address.port}/third`})).page.targetId;
 assert.equal((await action({kind:'tab',action:'close',tabId:second,url:null})).page.targetId,third);
 await action({kind:'tab',action:'close',tabId:third,url:null});


 const switched=await action({kind:'tab',action:'focus',tabId:first,url:null});assert.equal(switched.page.targetId,first);
 assert(!(await action({kind:'state'})).page.tabs.some(tab=>tab.tabId===second));
 await action({kind:'tab',action:'focus',tabId:first,url:null});
 await action({kind:'navigate',url:`http://127.0.0.1:${address.port}/new-document`});
 await assert.rejects(action({kind:'fill',element:'old-document-reference',text:'must be stale'},oldElement));
 const fresh=await action({kind:'state'});assert(fresh.elements.some(e=>e.attributes.id==='name'));
 const extra=await action({kind:'tab',action:'open',tabId:null,url:`http://127.0.0.1:${address.port}/closed-current`});
 // Simulate an external close without publishing a new current target to the caller.
 await operation({kind:'execute',action:{kind:'tab',action:'close',tabId:extra.page.targetId,url:null},targetId:extra.page.targetId,element:null,upload:null});
 await assert.rejects(action({kind:'navigate',url:`http://127.0.0.1:${address.port}/must-not-navigate`}));
 await assert.rejects(action({kind:'key',key:'Enter'}));
 await assert.rejects(action({kind:'scroll',deltaX:0,deltaY:100}));
 assert.equal((await action({kind:'state'})).page.targetId,first);

}
try {
 const initial=await action({kind:'state'});
 assert.match(initial.page.text??'',/Ready when you are/);
 const initialWebsite=webContents.fromId(Number(initial.page.targetId));assert(initialWebsite);
 assert.equal(await initialWebsite.executeJavaScript('typeof window.talosBrowser'),'undefined');
 const toolbar=webContents.getAllWebContents().find(contents=>contents.getURL().endsWith('/browser-window/index.html'));assert(toolbar);
 assert.equal(toolbar.debugger.isAttached(),false);
 await assert.rejects(action({kind:'tab',action:'focus',tabId:String(toolbar.id),url:null}));
 console.log('NATIVE_NAVIGATE');
 await action({kind:'navigate',url:`http://127.0.0.1:${address.port}`});
 console.log('NATIVE_NAVIGATED');
 let state=await action({kind:'state'});const input=state.elements.find(e=>e.attributes.id==='name');assert(input);
 await action({kind:'fill',element:'scoped-reference',text:'Worker'},input);
 state=await action({kind:'state'});const button=state.elements.find(e=>e.attributes.id==='hello');assert(button);
 await action({kind:'click',element:'scoped-reference'},button);
 state=await action({kind:'state'});assert.match(state.page.text??'',/Hello Worker/);
 const screenshot=await action({kind:'screenshot'});assert(screenshot.screenshot);mkdirSync(output,{recursive:true});writeFileSync(path.join(output,'native-browser.png'),Buffer.from(screenshot.screenshot,'base64'));
 const upload=state.elements.find(e=>e.attributes.id==='upload');assert(upload);
 await action({kind:'upload',element:'scoped-reference',fileId:'turn-owned'},upload,{filename:'upload.csv',dataBase64:Buffer.from('a,1\n').toString('base64')});
 state=await action({kind:'state'});assert.match(state.page.text??'',/upload.csv/);
 await downloadLifecycle();
 await assert.rejects(operation({kind:'execute',action:{kind:'click',element:'forged'},targetId,element:button,upload:null},'bob'));
 assert.equal(BrowserWindow.getAllWindows().length,0);
 for(const contents of webContents.getAllWebContents()){const prefs=contents.getLastWebPreferences();assert.equal(prefs.nodeIntegration,false);assert.equal(prefs.contextIsolation,true);assert.equal(prefs.sandbox,true);if(prefs.preload){assert.equal(prefs.preload,path.join(root,".tmp/browser-worker/native/dist/browser-window/preload.cjs"));assert.equal(contents.debugger.isAttached(),false);}}
 await tabLifecycle();
 // A detached dedicated target must fail explicitly, never redirect an old action.
 const detached=webContents.fromId(Number(targetId));assert(detached);detached.debugger.detach();
 await assert.rejects(action({kind:'state'}));
 await host.reset();targetId=null;
 await action({kind:'navigate',url:`http://127.0.0.1:${address.port}`});
 assert.match((await action({kind:'state'})).page.text??'',/Ready/);
 await operation({kind:'show'});await operation({kind:'releaseInputs'});
 if(process.env.TALOS_ACCEPTANCE_CHROME==='1') {
  console.log('PREPARED_CHROME_CONNECT_START');
  await operation({kind:'connect'});
  console.log('PREPARED_CHROME_CONNECT_READY');
  const status=await operation({kind:'probe'});assert.equal(status.kind,'readiness');if(status.kind==='readiness')assert.equal(status.connected,true);
  selectedTarget='connected';targetId=null;
  await action({kind:'navigate',url:`http://127.0.0.1:${address.port}`});
  let chromeState=await action({kind:'state'});const field=chromeState.elements.find(e=>e.attributes.id==='name');assert(field);
  await action({kind:'fill',element:'scoped-reference',text:'Chrome'},field);
  chromeState=await action({kind:'state'});const submit=chromeState.elements.find(e=>e.attributes.id==='hello');assert(submit);
  await action({kind:'click',element:'scoped-reference'},submit);
  chromeState=await action({kind:'state'});assert.match(chromeState.page.text??'',/Hello Chrome/);
  await action({kind:'screenshot'});
  await downloadLifecycle();
  await tabLifecycle();
  await operation({kind:'disconnect'});
  const remaining=await fetch('http://127.0.0.1:9222/json/version');assert.equal(remaining.ok,true);
  console.log('PREPARED_CHROME_ACCEPTANCE_PASS');
 }
 writeFileSync(path.join(output,'native-engine-acceptance.json'),JSON.stringify({navigate:true,dom:true,fill:true,click:true,screenshot:true,upload:true,download:true,downloadCollisionAndUuid:true,accountIsolation:true,tabs:true,staleTabAndDocumentReferences:true,debuggerDetachExplicitFailure:true,resetRecovery:true,sandbox:true,noProductRendererDebugger:true},null,2));
 console.log('NATIVE_ENGINE_ACCEPTANCE_PASS');
}finally{await host.reset();console.log('NATIVE_HOST_RESET');fixture.closeAllConnections();await new Promise<void>(resolve=>fixture.close(()=>resolve()));console.log('NATIVE_FIXTURE_CLOSED');app.quit();}

}
void run().catch(error=>{console.error(error);app.exit(1);});
