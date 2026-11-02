import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {mkdtempSync,rmSync} from "node:fs";
import {tmpdir} from "node:os";
import path from "node:path";
import type {AppProtocol} from "@talos/sdk-core";
const fixture=vi.hoisted(()=>({home:""}));
vi.mock("electron",()=>({app:{getPath:()=>fixture.home},BaseWindow:class{},WebContentsView:class{},session:{},ipcMain:{on:vi.fn(),removeListener:vi.fn()}}));
import {DesktopBrowserHost} from "./browser-host";
import {WebsiteAccount} from "./browser-website";
import {trackInputs,release,type RecordValue} from "./browser-input";
import {BrowserHostSocket as Socket} from "./browser-host.fixture";
const request=(kind:"connect"|"probe"):AppProtocol.BrowserHostRequest=>({userId:"alice",generation:"one",epoch:1,target:"connected",operation:{kind}});
let host:DesktopBrowserHost;
beforeEach(()=>{fixture.home=mkdtempSync(path.join(tmpdir(),"talos-input-"));host=new DesktopBrowserHost();Socket.all=[];Socket.targets=[];Socket.profileRoot=fixture.home;vi.stubGlobal("WebSocket",Socket);});
afterEach(async()=>{vi.useRealTimers();vi.restoreAllMocks();await host.reset();vi.unstubAllGlobals();rmSync(fixture.home,{recursive:true,force:true});});

async function inputFixture(target:"managed"|"connected",intercept:(method:string,params:RecordValue)=>Promise<void>|void,elementTag="button") {
  const inputs:RecordValue[]=[];
  const respond=async(method:string,params:RecordValue={})=>{
    if(method.startsWith("Input.dispatch"))inputs.push({...params});
    await intercept(method,params);
    if(method==="DOM.describeNode")return {node:{nodeName:elementTag.toUpperCase(),attributes:[]}};
    if(method==="DOM.resolveNode")return {object:{objectId:"fill-handle"}};
    if(method==="DOM.getContentQuads")return {quads:[[0,0,100,0,100,20,0,20]]};
    if(method==="DOM.querySelectorAll")return {nodeIds:[]};
    return {};
  };
  const send=trackInputs(respond);
  vi.spyOn(WebsiteAccount.prototype,"tabs").mockResolvedValue([{id:"input-tab",send,url:()=>"about:blank",title:()=>"Inputs"}]);
  vi.spyOn(WebsiteAccount.prototype,"releaseInputs").mockImplementation(()=>release(send));
  if(target==="connected") {
    vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
    await host.execute("connect",request("connect"));
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
  const execute=(id:string,action:AppProtocol.BrowserAction)=>host.execute(id,{...request("probe"),target,operation:{kind:"execute",targetId:"input-tab",upload:null,element:action.kind==="click"||action.kind==="fill"?{targetId:"input-tab",loaderId:"",backendNodeId:1,tag:elementTag,attributes:{}}:null,action}});
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

it.each(["managed","connected"] as const)("waits for the %s page to paint before dispatching its first click",async(target)=>{
  const waiting=deferred(),painted=deferred();
  const {inputs,execute}=await inputFixture(target,async(method,params)=>{
    if(method==="Runtime.evaluate"&&params.awaitPromise){waiting.resolve();await painted.promise;}
  });
  const clicking=execute("first",{kind:"click",element:"ref"});
  try {
    await waiting.promise;
    expect(inputs).toEqual([]);
  } finally {painted.resolve();await clicking;}
  expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
});

it.each(["managed","connected"] as const)("cancels %s input while the first page paint is pending",async(target)=>{
  const waiting=deferred(),painted=deferred();
  const {inputs,execute}=await inputFixture(target,async(method,params)=>{
    if(method==="Runtime.evaluate"&&params.awaitPromise){waiting.resolve();await painted.promise;}
  });
  const clicking=expect(execute("first",{kind:"click",element:"ref"})).rejects.toThrow("cancelled");
  try {
    await waiting.promise;host.cancel("first");await clicking;
    await execute("observe",{kind:"state"});
    expect(inputs).toEqual([]);
  } finally {painted.resolve();await clicking;}
});

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

it.each(["managed","connected"] as const)("releases cancelled %s input while its down acknowledgement is still pending",async(target)=>{
  const pressed=deferred(),acknowledged=deferred();
  const {inputs,execute}=await inputFixture(target,async(_method,params)=>{
    if(params.type==="mousePressed"){pressed.resolve();await acknowledged.promise;}
  });
  const interrupted=expect(execute("interrupted",{kind:"click",element:"ref"})).rejects.toThrow("cancelled");
  try {
    await pressed.promise;host.cancel("interrupted");
    await new Promise(resolve=>setImmediate(resolve));
    expect(inputs.map(input=>input.type)).toEqual(["mouseMoved","mousePressed","mouseReleased"]);
    await interrupted;
  } finally {acknowledged.resolve();await interrupted;}
});


it.each(["connect","releaseInputs","disconnect","shutdown"] as const)("recovers through %s after the prepared Chrome socket closes",async(kind)=>{
  vi.stubGlobal("fetch",vi.fn(async()=>({json:async()=>({Browser:"Chrome/140.0",webSocketDebuggerUrl:"ws://127.0.0.1:9222/devtools/browser/test"})})));
  await host.execute("connect",request("connect"));
  Socket.all[0].close();
  expect(await host.execute("probe",request("probe"))).toEqual({kind:"readiness",connected:false});
  await expect(host.execute("recover",{...request("probe"),operation:{kind}})).resolves.toEqual({kind:"done"});
  if(kind!=="connect")await host.execute("reconnect",request("connect"));
  expect(await host.execute("ready",request("probe"))).toEqual({kind:"readiness",connected:true});
});

it("keeps later browser operations available after a fill handle disposal fails", async()=>{
 const {execute}=await inputFixture("managed",method=>{if(method==="Runtime.releaseObjectGroup")throw Error("Target context closed");},"input");
 await expect(execute("retired-fill",{kind:"fill",element:"ref",text:"draft"})).rejects.toThrow("Target context closed");
 await expect(execute("next",{kind:"state"})).resolves.toMatchObject({kind:"observation"});
});
