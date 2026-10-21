import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {existsSync,mkdtempSync,readFileSync,readdirSync,rmSync,writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import path from "node:path";
import type {AppProtocol} from "@talos/sdk-core";
const fixture=vi.hoisted(()=>({home:""}));
vi.mock("electron",()=>({app:{getPath:()=>fixture.home},BaseWindow:class{},WebContentsView:class{},session:{},ipcMain:{on:vi.fn(),removeListener:vi.fn()}}));
import {DesktopBrowserHost} from "./browser-host";
import {WebsiteAccount,downloadRoot} from "./browser-website";
import {trackInputs,release,type RecordValue} from "./browser-input";
class Socket extends EventTarget {
  static OPEN=1;
  static all:Socket[]=[];
  static targets:Array<{targetId:string;type:string;url:string;title:string}>=[];
  readyState=0;
  commands:Array<{id:number;method:string;params?:Record<string,unknown>}>=[];
  constructor(){super();Socket.all.push(this);queueMicrotask(()=>{this.readyState=1;this.dispatchEvent(new Event("open"));});}
  send(text:string){
    const command=JSON.parse(text);this.commands.push(command);
    const result=command.method==="Browser.getBrowserCommandLine"?{arguments:["--enable-automation",`--user-data-dir=${fixture.home}`]}:command.method==="Target.getTargets"?{targetInfos:Socket.targets}:{};
    queueMicrotask(()=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,result})})));
  }
  close(){this.readyState=3;this.dispatchEvent(new Event("close"));}
}
const request=(kind:"connect"|"probe"):AppProtocol.BrowserHostRequest=>({userId:"alice",generation:"one",epoch:1,target:"connected",operation:{kind}});
let host:DesktopBrowserHost;
beforeEach(()=>{fixture.home=mkdtempSync(path.join(tmpdir(),"talos-chrome-"));host=new DesktopBrowserHost();Socket.all=[];Socket.targets=[];vi.stubGlobal("WebSocket",Socket);});
afterEach(async()=>{vi.useRealTimers();vi.restoreAllMocks();await host.reset();vi.unstubAllGlobals();rmSync(fixture.home,{recursive:true,force:true});});
function delayHandshake(){let finish!:()=>void;const pending=new Promise<Response>(resolve=>{finish=()=>resolve({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})} as Response);});return{pending,finish};}
it.each(["reset","cancel"])("closes a Chrome candidate completed after %s",async(action)=>{
  const delayed=delayHandshake();vi.stubGlobal("fetch",vi.fn(()=>delayed.pending));
  const operation=host.execute("old",request("connect"));const rejected=expect(operation).rejects.toThrow("cancelled");
  if(action==="reset")await host.reset();else host.cancel("old");
  delayed.finish();await rejected;
  expect(Socket.all).toHaveLength(1);expect(Socket.all[0].readyState).toBe(3);
  expect(Socket.all[0].commands.at(-1)).toMatchObject({method:"Browser.setDownloadBehavior",params:{behavior:"default"}});
  expect(await host.execute("probe",request("probe"))).toEqual({kind:"readiness",connected:false});
});
it("retires the old handshake before a new connection configures Chrome",async()=>{
  const first=delayHandshake(),second=delayHandshake();const fetch=vi.fn().mockReturnValueOnce(first.pending).mockReturnValueOnce(second.pending);vi.stubGlobal("fetch",fetch);
  const old=host.execute("old",request("connect"));const rejected=expect(old).rejects.toThrow("superseded");
  const current=host.execute("new",request("connect"));second.finish();expect(fetch).toHaveBeenCalledTimes(1);
  first.finish();await rejected;await current;
  expect(Socket.all.map(socket=>socket.readyState)).toEqual([3,1]);
  expect(await host.execute("probe",request("probe"))).toEqual({kind:"readiness",connected:true});
});

it("rejects actions for a closed current tab instead of selecting another tab",async()=>{
  vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
  await host.execute("connect",request("connect"));
  Socket.targets=[{targetId:"other-tab",type:"page",url:"about:blank",title:"Other tab"}];
  const actions:AppProtocol.BrowserAction[]=[{kind:"navigate",url:"https://example.test"},{kind:"key",key:"Enter"},{kind:"scroll",deltaX:0,deltaY:100}];
  for(const action of actions) {
    await expect(host.execute("stale",{...request("probe"),operation:{kind:"execute",action,targetId:"closed-tab",element:null,upload:null}})).rejects.toThrow("tab was closed");
  }
  expect(Socket.all[0].commands.some(command=>command.method.startsWith("Input.")||command.method==="Page.navigate")).toBe(false);
});

const downloadRequest=(target:"connected"|"managed"="connected"):AppProtocol.BrowserHostRequest=>({...request("probe"),target,operation:{kind:"execute",action:{kind:"download"},targetId:null,element:null,upload:null}});
async function connectedDownloads() {
  vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
  await host.execute("connect",request("connect"));
  Socket.targets=[{targetId:"download-tab",type:"page",url:"about:blank",title:"Downloads"}];
  const socket=Socket.all[0],directory=downloadRoot("alice-chrome");
  const emit=(method:string,params:Record<string,unknown>)=>socket.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({method,params})}));
  const begin=(guid:string,suggestedFilename:string)=>emit("Browser.downloadWillBegin",{guid,suggestedFilename});
  const complete=(guid:string)=>emit("Browser.downloadProgress",{guid,state:"completed"});
  return {directory,sourceDirectory:path.join(directory,".chrome-pending"),begin,complete};
}
async function collect(target:"connected"|"managed"="connected") {
  const response=await host.execute("download",downloadRequest(target));
  expect(response.kind).toBe("observation");
  if(response.kind!=="observation")throw Error("Expected observation.");
  return response.downloads;
}
it("collects both same-name Chrome downloads without overwriting and clears them only on receipts",async()=>{
  const {sourceDirectory,begin,complete}=await connectedDownloads();
  for(const [guid,content] of [["first-guid","first report"],["second-guid","second report"]]) {
    begin(guid,"report.pdf");writeFileSync(path.join(sourceDirectory,guid),content);complete(guid);
    expect(existsSync(path.join(sourceDirectory,guid))).toBe(false);
  }
  const files=await collect();
  expect(Object.fromEntries(files.map(file=>[file.filename,Buffer.from(file.dataBase64,"base64").toString()]))).toEqual({"report.pdf":"first report","report (1).pdf":"second report"});
  expect(await collect()).toEqual(files);
  const {createHash}=await import("node:crypto");
  await host.execute("receipt",{...request("probe"),operation:{kind:"acknowledgeDownloads",files:files.map(file=>({filename:file.filename,sha256:createHash("sha256").update(Buffer.from(file.dataBase64,"base64")).digest("hex")}))}});
  expect(await collect()).toEqual([]);
});
it("excludes actual in-progress GUIDs while returning legitimate completed UUID filenames",async()=>{
  const {sourceDirectory,begin,complete}=await connectedDownloads();
  const pending="11111111-1111-1111-1111-111111111111",filename="12345678-1234-1234-1234-123456789abc";
  begin(pending,"pending.pdf");writeFileSync(path.join(sourceDirectory,pending),"partial bytes");
  begin("completed-guid",filename);writeFileSync(path.join(sourceDirectory,"completed-guid"),"%PDF-1.7 completed");complete("completed-guid");
  expect(await collect()).toEqual([{filename,dataBase64:Buffer.from("%PDF-1.7 completed").toString("base64")}]);
  expect(existsSync(path.join(sourceDirectory,pending))).toBe(true);
});
it("keeps a failed completion visible and retries its GUID on the next collection",async()=>{
  const {directory,sourceDirectory,begin,complete}=await connectedDownloads();
  vi.spyOn(console,"error").mockImplementation(()=>{});
  begin("retry-guid","report.pdf");complete("retry-guid");
  await expect(collect()).rejects.toThrow("Retry collecting downloads");
  writeFileSync(path.join(sourceDirectory,"retry-guid"),"recovered report");
  expect(await collect()).toEqual([{filename:"report.pdf",dataBase64:Buffer.from("recovered report").toString("base64")}]);
  expect(readdirSync(directory)).toEqual([".chrome-pending","report.pdf"]);
  expect(readFileSync(path.join(directory,"report.pdf"),"utf8")).toBe("recovered report");
});
it("returns legitimate UUID filenames from the bundled browser",async()=>{
  const filename="12345678-1234-1234-1234-123456789abc",directory=downloadRoot("alice");
  writeFileSync(path.join(directory,filename),"%PDF-1.7 bundled");
  vi.spyOn(WebsiteAccount.prototype,"tabs").mockResolvedValue([{id:"bundled-tab",send:async()=>({}),url:()=>"about:blank",title:()=>"Downloads"}]);
  expect(await collect("managed")).toEqual([{filename,dataBase64:Buffer.from("%PDF-1.7 bundled").toString("base64")}]);
});

it("accepts a completed download whose legitimate UUID filename equals its CDP GUID",async()=>{
  const {directory,sourceDirectory,begin,complete}=await connectedDownloads();
  const filename="12345678-1234-1234-1234-123456789abc";
  begin(filename,filename);writeFileSync(path.join(sourceDirectory,filename),"%PDF-1.7 same GUID");complete(filename);
  expect(await collect()).toEqual([{filename,dataBase64:Buffer.from("%PDF-1.7 same GUID").toString("base64")}]);
  expect(readdirSync(directory)).toEqual([".chrome-pending",filename]);
});

it("keeps unfinished Chrome GUID sources private after reconnecting",async()=>{
  const {directory,sourceDirectory,begin}=await connectedDownloads();
  const guid="11111111-1111-1111-1111-111111111111",filename="12345678-1234-1234-1234-123456789abc";
  begin(guid,"unfinished.pdf");writeFileSync(path.join(sourceDirectory,guid),"incomplete source");
  writeFileSync(path.join(directory,filename),"%PDF-1.7 final file");
  await host.execute("reconnect",request("connect"));
  expect(await collect()).toEqual([{filename,dataBase64:Buffer.from("%PDF-1.7 final file").toString("base64")}]);
  expect(readFileSync(path.join(sourceDirectory,guid),"utf8")).toBe("incomplete source");
});

it.each([null,"other"])("closes the current tab by default and respects explicit tab %s",async(tabId)=>{
  const tab=(id:string)=>({id,send:async()=>({}),url:()=>"about:blank",title:()=>id});
  let tabs=[tab("other"),tab("current")];
  vi.spyOn(WebsiteAccount.prototype,"tabs").mockImplementation(async()=>tabs);
  const remove=vi.spyOn(WebsiteAccount.prototype,"remove").mockImplementation(async(id)=>{tabs=tabs.filter(tab=>tab.id!==id);});
  const operation:AppProtocol.BrowserHostRequest={...request("probe"),target:"managed",operation:{kind:"execute",targetId:"current",element:null,upload:null,action:{kind:"tab",action:"close",tabId,url:null}}};
  await host.execute("close",operation);
  expect(remove).toHaveBeenCalledWith(tabId??"current");
  await expect(host.execute("focus",{...operation,operation:{...operation.operation as Extract<AppProtocol.BrowserHostOperation,{kind:"execute"}>,action:{kind:"tab",action:"focus",tabId:null,url:null}}})).rejects.toThrow("Unknown browser tab");
});

it("waits for Chrome to retire a closing target before inspecting remaining tabs",async()=>{
  vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
  Socket.targets=["other","current"].map(targetId=>({targetId,type:"page",url:"about:blank",title:targetId}));
  const originalSend=Socket.prototype.send;
  let closing:string|undefined,closingSnapshots=0,reattachAttempts=0;
  vi.spyOn(Socket.prototype,"send").mockImplementation(function(this:Socket,text:string){
    const command=JSON.parse(text);
    let result:Record<string,unknown>|undefined,error:Record<string,unknown>|undefined;
    if(command.method==="Target.closeTarget") {closing=command.params.targetId;result={success:true};}
    else if(command.method==="Target.getTargets"&&closing) {
      closingSnapshots++;
      if(closingSnapshots>1)Socket.targets=Socket.targets.filter(target=>target.targetId!==closing);
      result={targetInfos:Socket.targets};
    } else if(command.method==="Target.attachToTarget"&&command.params.targetId===closing) {
      reattachAttempts++;error={code:-32000,message:"Target is closing"};
    } else {originalSend.call(this,text);return;}
    this.commands.push(command);
    queueMicrotask(()=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,...(error?{error}:{result})})})));
  });
  await host.execute("connect",request("connect"));
  const response=await host.execute("close",{...request("probe"),operation:{kind:"execute",targetId:"current",element:null,upload:null,action:{kind:"tab",action:"close",tabId:null,url:null}}});
  expect(response.kind).toBe("observation");
  if(response.kind!=="observation")throw Error("Expected remaining tab observation.");
  expect(response.page.targetId).toBe("other");
  expect(response.page.tabs.map(tab=>tab.tabId)).toEqual(["other"]);
  expect(closingSnapshots).toBeGreaterThan(1);
  expect(reattachAttempts).toBe(0);
});

it("fails explicitly when Chrome never removes an acknowledged closing target",async()=>{
  vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
  Socket.targets=[{targetId:"current",type:"page",url:"about:blank",title:"Current"}];
  await host.execute("connect",request("connect"));
  let now=0;vi.spyOn(Date,"now").mockImplementation(()=>now+=1500);
  await expect(host.execute("close",{...request("probe"),operation:{kind:"execute",targetId:"current",element:null,upload:null,action:{kind:"tab",action:"close",tabId:null,url:null}}})).rejects.toThrow("Chrome tab close timed out");
  expect(Socket.all[0].commands.filter(command=>command.method==="Target.closeTarget")).toHaveLength(1);
  expect(Socket.all[0].commands.filter(command=>command.method==="Target.attachToTarget")).toHaveLength(1);
  expect(Socket.all[0].commands.some(command=>command.method==="Page.enable")).toBe(false);
});

async function inputFixture(target:"managed"|"connected",intercept:(method:string,params:RecordValue)=>Promise<void>|void) {
  const inputs:RecordValue[]=[];
  const respond=async(method:string,params:RecordValue={})=>{
    if(method.startsWith("Input.dispatch"))inputs.push({...params});
    await intercept(method,params);
    if(method==="DOM.describeNode")return {node:{nodeName:"BUTTON",attributes:[]}};
    if(method==="DOM.getContentQuads")return {quads:[[0,0,100,0,100,20,0,20]]};
    if(method==="DOM.querySelectorAll")return {nodeIds:[]};
    return {};
  };
  const send=trackInputs(respond);
  vi.spyOn(WebsiteAccount.prototype,"tabs").mockResolvedValue([{id:"input-tab",send,url:()=>"about:blank",title:()=>"Inputs"}]);
  vi.spyOn(WebsiteAccount.prototype,"releaseInputs").mockImplementation(()=>release(send));
  if(target==="connected") {
    await connectedDownloads();
    Socket.targets=[{targetId:"input-tab",type:"page",url:"about:blank",title:"Inputs"}];
    const original=Socket.prototype.send;
    vi.spyOn(Socket.prototype,"send").mockImplementation(function(this:Socket,text:string){
      const command=JSON.parse(text);
      if(command.method.startsWith("Target.")||command.method.startsWith("Browser.")){original.call(this,text);return;}
      this.commands.push(command);
      void respond(command.method,command.params).then(
        result=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,result})})),
        ()=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,error:{code:-32000,message:"Injected CDP failure"}})})),
      );
    });
  }
  const execute=(id:string,action:AppProtocol.BrowserAction)=>host.execute(id,{...request("probe"),target,operation:{kind:"execute",targetId:"input-tab",upload:null,element:action.kind==="click"?{targetId:"input-tab",loaderId:"",backendNodeId:1,tag:"button",attributes:{}}:null,action}});
  return {inputs,execute,release:()=>host.execute("release",{...request("probe"),target,operation:{kind:"releaseInputs"}})};
}

it.each(["managed","connected"] as const)("releases %s mouse input when cancelled after mouse down",async(target)=>{
  const {inputs,execute}=await inputFixture(target,(_method,params)=>{if(params.type==="mousePressed")host.cancel("interrupted");});
  await expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("cancelled");
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
  await execute("next",{kind:"state"});
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
});

it.each(["managed","connected"] as const)("releases %s keyboard input after a failed key-down command",async(target)=>{
  const {inputs,execute}=await inputFixture(target,(_method,params)=>{if(params.type==="keyDown")throw Error("Injected CDP failure");});
  await expect(execute("interrupted",{kind:"key",key:"Shift+Enter"})).rejects.toThrow();
  expect(inputs).toEqual([expect.objectContaining({type:"keyDown",key:"Enter",modifiers:8}),expect.objectContaining({type:"keyUp",key:"Enter",modifiers:0})]);
});

function deferred() {let resolve!:()=>void;const promise=new Promise<void>(done=>{resolve=done;});return {promise,resolve};}

it.each(["managed","connected"] as const)("fences subsequent %s input until cancelled input release is acknowledged",async(target)=>{
  const releasing=deferred(),acknowledged=deferred();
  const {inputs,execute}=await inputFixture(target,async(_method,params)=>{
    if(params.type==="mousePressed")host.cancel("interrupted");
    if(params.type==="mouseReleased"){releasing.resolve();await acknowledged.promise;}
  });
  const interrupted=expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("cancelled");
  await releasing.promise;
  const next=execute("next",{kind:"key",key:"Enter"});
  await new Promise(resolve=>setImmediate(resolve));
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
  acknowledged.resolve();
  await interrupted;await next;
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased","keyDown","keyUp"]);
});

it.each(["managed","connected"] as const)("blocks %s input after cleanup failure until explicit release succeeds",async(target)=>{
  let fail=true;
  const {inputs,execute,release:retryRelease}=await inputFixture(target,(_method,params)=>{
    if(params.type==="mousePressed")host.cancel("interrupted");
    if(params.type==="mouseReleased"&&fail)throw Error("Injected release failure");
  });
  await expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("input cleanup failed");
  const before=inputs.length;
  await expect(execute("blocked",{kind:"key",key:"Enter"})).rejects.toThrow("input cleanup failed");
  expect(inputs).toHaveLength(before);
  fail=false;await retryRelease();await execute("recovered",{kind:"key",key:"Enter"});
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased","mouseReleased","keyDown","keyUp"]);
});

it("bounds a stalled release and keeps recovery fenced until the original cleanup settles",async()=>{
  const releasing=deferred(),acknowledged=deferred();
  const {inputs,execute,release:retryRelease}=await inputFixture("managed",async(_method,params)=>{
    if(params.type==="mousePressed")host.cancel("interrupted");
    if(params.type==="mouseReleased"){releasing.resolve();await acknowledged.promise;}
  });
  vi.useFakeTimers({toFake:["setTimeout","clearTimeout"]});
  const interrupted=expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("input cleanup failed");
  await releasing.promise;
  await vi.advanceTimersByTimeAsync(3001);await interrupted;
  await expect(execute("blocked",{kind:"key",key:"Enter"})).rejects.toThrow("input cleanup failed");
  let recovered=false;
  const recovery=retryRelease().then(()=>{recovered=true;});
  await vi.advanceTimersByTimeAsync(0);
  expect(recovered).toBe(false);
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
  acknowledged.resolve();await recovery;
  await execute("recovered",{kind:"key",key:"Enter"});
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased","keyDown","keyUp"]);
});

it.each(["managed","connected"] as const)("drains cancelled %s input before reset retires its targets",async(target)=>{
  const releasing=deferred(),acknowledged=deferred();
  const {execute}=await inputFixture(target,async(_method,params)=>{
    if(params.type==="mousePressed")host.cancel("interrupted");
    if(params.type==="mouseReleased"){releasing.resolve();await acknowledged.promise;}
  });
  const close=vi.spyOn(WebsiteAccount.prototype,"close");
  const interrupted=expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("cancelled");
  await releasing.promise;
  const resetting=host.reset();let retired=false;void resetting.then(()=>{retired=true;});
  await new Promise(resolve=>setImmediate(resolve));
  expect(retired).toBe(false);expect(close).not.toHaveBeenCalled();
  if(target==="connected")expect(Socket.all[0].readyState).toBe(1);
  acknowledged.resolve();await interrupted;await resetting;
  expect(close).toHaveBeenCalledOnce();
  if(target==="connected")expect(Socket.all[0].readyState).toBe(3);
});

it("retains the connected input owner when reconnect cleanup fails so release can be retried",async()=>{
  let fail=true;
  const {execute,release:retryRelease}=await inputFixture("connected",(_method,params)=>{
    if(params.type==="mousePressed")host.cancel("interrupted");
    if(params.type==="mouseReleased"&&fail)throw Error("Injected release failure");
  });
  await expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("input cleanup failed");
  await expect(host.execute("reconnect",request("connect"))).rejects.toThrow("input cleanup failed");
  expect(Socket.all).toHaveLength(1);expect(Socket.all[0].readyState).toBe(1);
  fail=false;await retryRelease();await host.execute("reconnect",request("connect"));
  expect(Socket.all.map(socket=>socket.readyState)).toEqual([3,1]);
});
