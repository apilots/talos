import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {appendFileSync,mkdirSync,writeFileSync,rmSync} from 'node:fs';
import path from 'node:path';
import {createHash,randomUUID} from 'node:crypto';
import {app,BaseWindow,BrowserWindow,webContents,type WebContentsView} from 'electron';
import type {AppProtocol} from '@talos/sdk-core';
import {DesktopBrowserHost} from '../../../talos-app/desktop/src/browser-host';
import {createDesktopWindow} from '../../../talos-app/desktop/src/desktop-window';
const root=process.env.TALOS_RESOURCE_DIR!;
const output=process.env.TALOS_SCENARIO_REPORT_DIR??path.join(root,'output/browser-worker');
mkdirSync(output,{recursive:true});
const tracePath=path.join(output,'native-engine-cdp.jsonl');
writeFileSync(tracePath,'');
for(const name of ['native-engine-acceptance.json','native-engine-modes.json'])rmSync(path.join(output,name),{force:true});
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
app.setPath('userData',path.join(process.env.TALOS_HOME??path.join(import.meta.dirname,'..'),'profile-'+randomUUID()));
async function run(){
console.log('NATIVE_START');
await app.whenReady();
console.log('NATIVE_READY');
const uuidFilename='12345678-1234-1234-1234-123456789abc';
let finishPendingReport:(()=>void)|undefined;
const fixture=createServer((req,res)=>{const pathname=new URL(req.url??'/', 'http://127.0.0.1').pathname;trace({phase:'httpRequest',pathname});res.once('finish',()=>trace({phase:'httpResponseFinished',pathname,status:res.statusCode}));if(req.url==='/uuid'){res.writeHead(200,{'content-type':'application/pdf','content-disposition':`attachment; filename="${uuidFilename}"`});res.end('%PDF-1.7 UUID download');return;}if(req.url==='/report.csv'||req.url==='/report.csv?part=2'){res.writeHead(200,{'content-type':'text/csv','content-disposition':'attachment; filename="report.csv"'});if(req.url.endsWith('?part=2')){res.write('name,value\nworker,');finishPendingReport=()=>{res.end('2\n');finishPendingReport=undefined;};}else res.end('name,value\nworker,1\n');return;}res.writeHead(200,{'content-type':'text/html'});res.end(`<!doctype html><title>Talos browser acceptance</title><input id="name" placeholder="Name" value="Existing draft"><textarea id="notes">Existing notes</textarea><div id="editable" contenteditable="true">Existing rich text</div><button id="hello" onclick="document.querySelector('h1').textContent='Hello '+document.querySelector('#name').value">Hello</button><h1>Ready</h1><input id="upload" type="file" onchange="document.querySelector('h2').textContent=this.files[0].name"><h2></h2><a id="download" href="/report.csv">Download</a><a id="second-download" href="/report.csv?part=2">Second report</a><a id="uuid-download" href="/uuid">UUID download</a><script>window.events=[];document.addEventListener("keydown",event=>{if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==="a")event.preventDefault();});for(const type of ["mousedown","mouseup","click","pointerdown","keydown","wheel","input"])document.addEventListener(type,event=>window.events.push({type,x:event.clientX,y:event.clientY,target:event.target.id,key:event.key,trusted:event.isTrusted}));</script>`);});
await new Promise<void>(resolve=>fixture.listen(0,'127.0.0.1',resolve));
const address=fixture.address();assert(address&&typeof address!=='string');
const host=new DesktopBrowserHost(),generation=randomUUID();
const {window:product,view:productView}=createDesktopWindow({width:1180,height:840,show:false},{sandbox:true,contextIsolation:true,nodeIntegration:false});
const productContents=productView.webContents;
const rendererUrl=`http://127.0.0.1:${address.port}`;
await productContents.loadURL(rendererUrl);
host.workspace.setWindow(product,productView,rendererUrl);assert(host.workspace.accepts(productContents));host.workspace.bind('alice');
let targetId:string|null=null;let selectedTarget:AppProtocol.BrowserTargetKind='managed';
const modes:Partial<Record<AppProtocol.BrowserTargetKind,Record<string,unknown>>>={};
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
 const closedCurrent=await action({kind:'tab',action:'close',tabId:null,url:null});
 assert(!closedCurrent.page.tabs.some(tab=>tab.tabId===third));


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
 assert.equal(productContents.debugger.isAttached(),false);
 await assert.rejects(action({kind:'tab',action:'focus',tabId:String(productContents.id),url:null}));
 console.log('NATIVE_NAVIGATE');
 await action({kind:'navigate',url:`http://127.0.0.1:${address.port}`});
 console.log('NATIVE_NAVIGATED');
 let state=await action({kind:'state'});const input=state.elements.find(e=>e.attributes.id==='name');assert(input);
 await action({kind:'fill',element:'scoped-reference',text:'temporary value'},input);
 await action({kind:'fill',element:'scoped-reference',text:'Worker'},input);
 const filledWebsite=webContents.fromId(Number(targetId));assert(filledWebsite);
 for(const [id,text] of [['notes','Replaced notes'],['editable','Replaced rich text']]) {
  const editor=state.elements.find(element=>element.attributes.id===id);assert(editor);
  await action({kind:'fill',element:'scoped-reference',text:'temporary value'},editor);
  await action({kind:'fill',element:'scoped-reference',text},editor);
  assert.equal(await filledWebsite.executeJavaScript(`document.getElementById(${JSON.stringify(id)}).value ?? document.getElementById(${JSON.stringify(id)}).textContent`),text);
 }
 const inputEvents=await filledWebsite.executeJavaScript('window.events.filter(event=>event.type==="input")');
 for(const id of ['name','notes','editable'])assert(inputEvents.some((event:{target:string;trusted:boolean})=>event.target===id&&event.trusted),`Missing trusted input event for ${id}`);
 state=await action({kind:'state'});const button=state.elements.find(e=>e.attributes.id==='hello');assert(button);
 await action({kind:'click',element:'scoped-reference'},button);
 state=await action({kind:'state'});assert.match(state.page.text??'',/Hello Worker/);
 const screenshot=await action({kind:'screenshot'});assert(screenshot.screenshot);mkdirSync(output,{recursive:true});writeFileSync(path.join(output,'native-browser.png'),Buffer.from(screenshot.screenshot,'base64'));
 const upload=state.elements.find(e=>e.attributes.id==='upload');assert(upload);
 await action({kind:'upload',element:'scoped-reference',fileId:'turn-owned'},upload,{filename:'upload.csv',dataBase64:Buffer.from('a,1\n').toString('base64')});
 state=await action({kind:'state'});assert.match(state.page.text??'',/upload.csv/);
 await downloadLifecycle();
 await assert.rejects(operation({kind:'execute',action:{kind:'click',element:'forged'},targetId,element:button,upload:null},'bob'));
 assert.equal(BaseWindow.getAllWindows().length,1);
 for(const contents of webContents.getAllWebContents()){const prefs=contents.getLastWebPreferences();assert.equal(prefs.nodeIntegration,false);assert.equal(prefs.contextIsolation,true);assert.equal(prefs.sandbox,true);assert.equal(prefs.preload,undefined);}
 await tabLifecycle();
 // Real target-scoped input must work while another owned window has focus and
 // while the product is actually minimized. No user's browser/profile is borrowed.
 const office=new BrowserWindow({width:500,height:300,show:false,webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false}});
 await office.loadURL('data:text/html,<title>Owned office fixture</title><input id="office" autofocus value="untouched">');
 const until=async(test:()=>boolean|Promise<boolean>,label:string)=>{const deadline=Date.now()+10000;while(!await test()){if(Date.now()>deadline)throw Error(label);await new Promise(resolve=>setTimeout(resolve,20));}};
 try {
  product.showInactive();office.show();app.focus({steal:true});office.focus();await until(()=>office.isFocused(),'Owned office did not receive focus');
  const windows=BaseWindow.getAllWindows().length;
  const binding=host.workspace.bind('alice');
  await host.workspace.request({kind:'present',sessionId:binding.sessionId,tabId:targetId,bounds:{x:650,y:120,width:480,height:600}});
  const topContents=()=>(product.contentView.children.at(-1) as WebContentsView).webContents;
  console.log("NATIVE_STACKING",JSON.stringify({targetId,children:product.contentView.children.map(view=>({product:view===productView,id:(view as WebContentsView).webContents?.id}))}));
  assert.equal(String(topContents()?.id),targetId);
  for(const mode of ['shown-in-background','collapsed','minimized']) {
   if(mode==='collapsed'){await host.workspace.request({kind:'present',sessionId:binding.sessionId,tabId:null,bounds:null});assert.equal(product.contentView.children.at(-1),productView);}
   if(mode==='minimized'){product.minimize();await until(()=>product.isMinimized(),'Product did not minimize');office.focus();await until(()=>office.isFocused(),'Office focus was not retained');}
   await action({kind:'navigate',url:`http://127.0.0.1:${address.port}/background-${mode}`});
   let observation=await action({kind:'state'});const field=observation.elements.find(e=>e.attributes.id==='name');assert(field);
   await action({kind:'fill',element:'fixture',text:mode},field);
   await action({kind:'key',key:'Enter'});
   await action({kind:'key',key:process.platform==='darwin'?'Meta+w':'Control+w'});
   observation=await action({kind:'state'});const submit=observation.elements.find(e=>e.attributes.id==='hello');assert(submit);
   await action({kind:'click',element:'fixture'},submit);
   assert.match((await action({kind:'state'})).page.text??'',new RegExp('Hello '+mode));
   await action({kind:'scroll',deltaX:0,deltaY:100});
   const image=await action({kind:'screenshot'});assert(image.screenshot);writeFileSync(path.join(output,'native-background-'+mode+'.png'),Buffer.from(image.screenshot,'base64'));
   const website=webContents.fromId(Number(targetId));assert(website);
   assert.equal(await website.executeJavaScript('window.events.some(event=>event.type==="keydown"&&event.key==="Enter")'),true);
   await until(()=>website.executeJavaScript('window.events.some(event=>event.type==="wheel")'),"Target wheel event was not delivered");
   assert.equal(office.isFocused(),true,mode+' stole the office window focus');assert.equal(product.isFocused(),false);
   assert.equal(await office.webContents.executeJavaScript('document.querySelector("input").value'),'untouched');
   assert.equal(BaseWindow.getAllWindows().length,windows);
   if(mode==='minimized')assert.equal(product.isMinimized(),true);
   console.log('MANAGED_BACKGROUND_PASS '+mode);
  }
  // Restore must reattach the existing target and preserve its form state.
  const existingId=targetId;product.restore();await until(()=>!product.isMinimized(),'Product did not restore');
  office.focus();await until(()=>office.isFocused(),'Office focus was not retained after restore');
  await host.workspace.request({kind:'present',sessionId:binding.sessionId,tabId:existingId,bounds:{x:650,y:120,width:480,height:600}});
  assert.equal(String(topContents()?.id),existingId);assert.match((await action({kind:'state'})).page.text??'',/Hello minimized/);
  product.minimize();await until(()=>product.isMinimized(),'Product did not minimize again');office.focus();await until(()=>office.isFocused(),'Office focus was not retained');
  const website=webContents.fromId(Number(targetId));assert(website);
  // Native window managers finish restoring/minimizing geometry asynchronously.
  let lastBounds=JSON.stringify(product.getBounds()),settledAt=Date.now();
  await until(()=>{const current=JSON.stringify(product.getBounds());if(current!==lastBounds){lastBounds=current;settledAt=Date.now();}return Date.now()-settledAt>=150;},'Native minimize geometry did not settle');
  const originalBounds=product.getBounds();
  await website.executeJavaScript('alert("No native modal");window.focus();window.resizeTo(400,400);window.moveTo(0,0)');
  assert.deepEqual({bounds:product.getBounds(),officeFocused:office.isFocused(),productFocused:product.isFocused(),minimized:product.isMinimized()}, {bounds:originalBounds,officeFocused:true,productFocused:false,minimized:true});
  const before=(await action({kind:'state'})).page.tabs.length;
  await website.executeJavaScript(`window.open('http://127.0.0.1:${address.port}/popup')`);
  let popupState=await action({kind:'state'});
  for(let attempt=0;attempt<50&&popupState.page.tabs.length===before;attempt++){await new Promise(resolve=>setTimeout(resolve,20));popupState=await action({kind:'state'});}
  assert.equal(popupState.page.tabs.length,before+1);assert.equal(BaseWindow.getAllWindows().length,windows);assert(office.isFocused());assert(product.isMinimized());
  for(let attempt=0;attempt<50&&!popupState.page.tabs.some(tab=>tab.url.endsWith('/popup'));attempt++){await new Promise(resolve=>setTimeout(resolve,20));popupState=await action({kind:'state'});}
  const popup=popupState.page.tabs.find(tab=>tab.url.endsWith('/popup'));assert(popup);
  const opened=await operation({kind:'execute',action:{kind:'state'},targetId:popup.tabId,element:null,upload:null});assert(opened.kind==='observation');assert.match(opened.page.text??'',/Ready/);

  await website.executeJavaScript('document.cookie="talos_login=retained; path=/"; localStorage.setItem("talos_login","retained")');
  await website.session.flushStorageData();await website.session.cookies.flushStore();
  const otherAccount=await operation({kind:'execute',action:{kind:'navigate',url:`http://127.0.0.1:${address.port}/other-account`},targetId:null,element:null,upload:null},'bob');assert(otherAccount.kind==='observation');
  const otherWebsite=webContents.fromId(Number(otherAccount.page.targetId));assert(otherWebsite);
  assert.equal((await otherWebsite.session.cookies.get({url:`http://127.0.0.1:${address.port}`})).some(cookie=>cookie.name==='talos_login'),false);

  await host.reset();targetId=null;
  await action({kind:'navigate',url:`http://127.0.0.1:${address.port}/restored`});
  const restored=webContents.fromId(Number(targetId));assert(restored);
  assert.match(await restored.executeJavaScript('document.cookie'),/talos_login=retained/);
  assert.equal(await restored.executeJavaScript('localStorage.getItem("talos_login")'),'retained');
  assert((await action({kind:'screenshot'})).screenshot,'A new target created while minimized must paint');
  assert.equal(typeof await restored.executeJavaScript('window.talos'),'undefined');
  assert(office.isFocused());assert(product.isMinimized());
  console.log('MANAGED_POPUP_PROFILE_PASS');
 } finally {office.destroy();}
 const detached=webContents.fromId(Number(targetId));assert(detached);detached.debugger.detach();
 await assert.rejects(action({kind:'state'}));
 await host.reset();targetId=null;
 await action({kind:'navigate',url:`http://127.0.0.1:${address.port}`});
 assert.match((await action({kind:'state'})).page.text??'',/Ready/);
 await operation({kind:'show'});await operation({kind:'releaseInputs'});
 modes.managed={status:'passed',target:'managed',pageScreenshot:'native-browser.png',screenshotScope:'pageContent',dom:true,upload:true,downloadCollisionAndUuid:true,tabs:true};
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
  const chromeScreenshot=await action({kind:'screenshot'});assert(chromeScreenshot.screenshot);
  writeFileSync(path.join(output,'native-browser-connected.png'),Buffer.from(chromeScreenshot.screenshot,'base64'));
  const chromeUpload=chromeState.elements.find(e=>e.attributes.id==='upload');assert(chromeUpload);
  await action({kind:'upload',element:'scoped-reference',fileId:'turn-owned'},chromeUpload,{filename:'upload.csv',dataBase64:Buffer.from('a,1\n').toString('base64')});
  assert.match((await action({kind:'state'})).page.text??'',/upload.csv/);
  await downloadLifecycle();
  await tabLifecycle();
  await operation({kind:'disconnect'});
  const remaining=await fetch('http://127.0.0.1:9222/json/version');assert.equal(remaining.ok,true);
  modes.connected={status:'passed',target:'connected',pageScreenshot:'native-browser-connected.png',screenshotScope:'pageContent',dom:true,upload:true,downloadCollisionAndUuid:true,tabs:true,ownedChromeSurvivesDisconnect:true};
  console.log('PREPARED_CHROME_ACCEPTANCE_PASS');
 }
 writeFileSync(path.join(output,'native-engine-modes.json'),JSON.stringify(modes,null,2));
 writeFileSync(path.join(output,'native-engine-acceptance.json'),JSON.stringify({navigate:true,dom:true,fill:true,click:true,screenshot:true,upload:true,download:true,downloadCollisionAndUuid:true,accountIsolation:true,tabs:true,staleTabAndDocumentReferences:true,debuggerDetachExplicitFailure:true,resetRecovery:true,sandbox:true,noProductRendererDebugger:true,backgroundInputAndScreenshot:true,minimizedWithoutFocus:true,popupWithoutWindow:true,profileRetained:true,nativeStacking:true,dialogsAndMenuIsolation:true,restoreRetainsTarget:true,freshMinimizedTargetCapture:true},null,2));
 console.log('NATIVE_ENGINE_ACCEPTANCE_PASS');
}finally{await host.reset();product.destroy();console.log('NATIVE_HOST_RESET');fixture.closeAllConnections();await new Promise<void>(resolve=>fixture.close(()=>resolve()));console.log('NATIVE_FIXTURE_CLOSED');app.quit();}

}
void run().catch(error=>{console.error(error);app.exit(1);});
