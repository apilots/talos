import {afterEach,beforeEach,expect,it,vi} from "vitest";
import {existsSync,mkdtempSync,readdirSync,rmSync,truncateSync,writeFileSync} from "node:fs";
import {EventEmitter} from "node:events";
import {createHash} from "node:crypto";
import type {AppProtocol} from "@talos/sdk-core";
import {tmpdir} from "node:os";
import path from "node:path";
const fixture=vi.hoisted(()=>({capture:undefined as (()=>Promise<unknown>)|undefined,home:"",sessions:new Map<string,unknown>(),states:[] as unknown[],load:undefined as ((url:string)=>Promise<void>)|undefined}));
vi.mock("electron",async()=>{
  const {EventEmitter}=await import("node:events");let nextId=0;
  class Contents extends EventEmitter {
    id=++nextId;url="";destroyed=false;
    debugger={attach:vi.fn(),sendCommand:async()=>({})};
    capturePage=vi.fn(()=>fixture.capture?.()??Promise.resolve({isEmpty:()=>false,toPNG:()=>Buffer.from("fixture-image")}));
    navigationHistory={canGoBack:()=>false,canGoForward:()=>false};
    setIgnoreMenuShortcuts=vi.fn();
    setBackgroundThrottling(){}
    setWindowOpenHandler(){}getURL(){return this.url;}getTitle(){return "Downloads";}isLoading(){return false;}isDestroyed(){return this.destroyed;}
    async loadURL(url:string){this.url=url;await fixture.load?.(url);this.emit("did-finish-load");}
    send(_channel:string,value:unknown){fixture.states.push(value);}
    close(){this.destroyed=true;this.emit("destroyed");}
  }
  class View {
    children:unknown[]=[];
    addChildView(view:unknown,index=this.children.length){this.removeChildView(view);this.children.splice(index,0,view);}
    removeChildView(view:unknown){this.children=this.children.filter(child=>child!==view);}
    setBounds(){}
  }
  class Window extends EventEmitter {
    webContents=new Contents();
    contentView=new View();
    minimized=false;isMinimized(){return this.minimized;}
    isDestroyed(){return false;}
  }
  return {app:{getPath:()=>fixture.home},BrowserWindow:Window,WebContentsView:class{webContents=new Contents();setBounds(){}},session:{fromPartition:vi.fn((partition:string)=>{
    let value=fixture.sessions.get(partition);
    if(!value){value=Object.assign(new EventEmitter(),{setPermissionRequestHandler(){}});fixture.sessions.set(partition,value);}
    return value;
  })}};
});
import {WebsiteAccount,checkedUrl,downloadRoot} from "./browser-website";
import {DesktopBrowserHost} from "./browser-host";
import {BrowserWindow,type WebContentsView} from "electron";
let host:DesktopBrowserHost;
beforeEach(()=>{fixture.home=mkdtempSync(path.join(tmpdir(),"talos-website-"));fixture.sessions.clear();fixture.states=[];fixture.load=undefined;fixture.capture=undefined;host=new DesktopBrowserHost();const window=new BrowserWindow();host.workspace.setWindow(window,{webContents:window.webContents} as WebContentsView,"file:///product.html");host.workspace.bind("alice");});
afterEach(async()=>{vi.useRealTimers();await host.reset();vi.restoreAllMocks();rmSync(fixture.home,{recursive:true,force:true});});
it("rejects unsafe navigation schemes",()=>{
  for(const url of ["file:///etc/passwd","javascript:alert(1)","https://user:secret@example.test"])expect(()=>checkedUrl(url)).toThrow();
  expect(checkedUrl("https://talos.love")).toBe("https://talos.love/");
});

class Download extends EventEmitter {
  savePath="";receivedBytes=0;
  constructor(readonly filename:string){super();}
  getFilename(){return this.filename;}
  getReceivedBytes(){return this.receivedBytes;}
  setSavePath(value:string){this.savePath=value;}
  cancel=vi.fn(()=>this.emit("done",{},"cancelled"));
  finish(state="completed"){this.emit("done",{},state);}
}
const request=(operation:AppProtocol.BrowserHostOperation):AppProtocol.BrowserHostRequest=>({userId:"alice",generation:"one",epoch:1,target:"managed",operation});
async function collect() {
  const response=await host.execute("collect",request({kind:"execute",action:{kind:"download"},targetId:null,element:null,upload:null}));
  if(response.kind!=="observation")throw Error("Expected browser observation.");
  return response.downloads;
}
async function downloads(){
  expect(await collect()).toEqual([]);
  const browserSession=[...fixture.sessions.values()][0] as EventEmitter;
  const begin=(filename:string,content:string)=>{
    const item=new Download(filename);browserSession.emit("will-download",{},item);
    writeFileSync(item.savePath,content);return item;
  };
  return {browserSession,begin,directory:downloadRoot("alice")};
}
it("publishes distinct same-name contents and UUID files only after completion and keeps them until receipts",async()=>{
  const {begin,directory}=await downloads(),uuid="12345678-1234-1234-1234-123456789abc";
  const first=begin("report.pdf","first report"),second=begin("report.pdf","second report"),namedUuid=begin(uuid,"%PDF-1.7 UUID");
  expect(first.savePath).not.toBe(second.savePath);
  expect(path.dirname(first.savePath)).not.toBe(directory);
  expect(await collect()).toEqual([]);
  first.finish();
  expect(await collect()).toEqual([{filename:"report.pdf",dataBase64:Buffer.from("first report").toString("base64")}]);
  expect(existsSync(second.savePath)).toBe(true);
  second.finish();namedUuid.finish();
  const files=await collect();
  expect(Object.fromEntries(files.map(file=>[file.filename,Buffer.from(file.dataBase64,"base64").toString()]))).toEqual({"report.pdf":"first report","report (1).pdf":"second report",[uuid]:"%PDF-1.7 UUID"});
  expect(await collect()).toEqual(files);
  await host.execute("receipts",request({kind:"acknowledgeDownloads",files:files.map(file=>({filename:file.filename,sha256:createHash("sha256").update(Buffer.from(file.dataBase64,"base64")).digest("hex")}))}));
  expect(await collect()).toEqual([]);expect(readdirSync(directory)).toEqual([]);
});
it.each(["interrupted","cancelled"])("does not publish a %s download and exposes its failure",async(state)=>{
  const {begin,directory}=await downloads(),item=begin("report.pdf","partial bytes");
  item.finish(state);
  expect(await collect()).toEqual([]);expect(readdirSync(directory)).toEqual([]);
  expect(fixture.states.at(-1)).toMatchObject({error:"The download did not complete. Try downloading the file again."});
});
it("cancels oversized updates and cancels owned pending downloads when its account closes",async()=>{
  const {begin,browserSession,directory}=await downloads(),oversized=begin("large.pdf","partial bytes");
  oversized.receivedBytes=20*1024*1024+1;oversized.emit("updated");
  expect(oversized.cancel).toHaveBeenCalledOnce();expect(await collect()).toEqual([]);
  const pending=begin("report.pdf","pending bytes");
  await host.reset();
  expect(pending.cancel).toHaveBeenCalledOnce();expect(readdirSync(directory)).toEqual([]);
  expect(browserSession.listenerCount("will-download")).toBe(0);
  expect(await collect()).toEqual([]);
  expect(browserSession.listenerCount("will-download")).toBe(1);
});

it("rejects oversized completed bytes and lets the user retry with a fresh download",async()=>{
  const {begin,directory}=await downloads(),oversized=begin("report.pdf","partial bytes");
  vi.spyOn(console,"error").mockImplementation(()=>{});
  truncateSync(oversized.savePath,20*1024*1024+1);oversized.finish();
  expect(await collect()).toEqual([]);expect(readdirSync(directory)).toEqual([]);
  expect(fixture.states.at(-1)).toMatchObject({error:"The download could not be saved. Try downloading the file again."});
  const retry=begin("report.pdf","successful retry");retry.finish();
  expect(await collect()).toEqual([{filename:"report.pdf",dataBase64:Buffer.from("successful retry").toString("base64")}]);
  expect(fixture.states.at(-1)).toMatchObject({error:null});
});

it.each(["home","page"] as const)("reset interrupts a stalled managed %s load and prevents late target recreation",async(kind)=>{
  let started!:()=>void,finish!:()=>void;
  const loading=new Promise<void>(resolve=>{started=resolve;}),loaded=new Promise<void>(resolve=>{finish=resolve;});
  const load=vi.fn(async(url:string)=>{
    if(kind==="home"?url.endsWith("/home/home.html"):url==="https://pending.test/"){started();await loaded;}
  });
  fixture.load=load;
  const close=vi.spyOn(WebsiteAccount.prototype,"close");
  const action=host.execute("loading",request({kind:"execute",action:kind==="home"?{kind:"state"}:{kind:"tab",action:"open",url:"https://pending.test/",tabId:null},targetId:null,element:null,upload:null}));
  const cancelled=expect(action).rejects.toThrow("cancelled");
  await loading;
  let reset=false;
  const resetting=host.reset().then(()=>{reset=true;});
  const count=load.mock.calls.length;
  try {
    await new Promise(resolve=>setImmediate(resolve));
    expect(reset).toBe(true);expect(close).toHaveBeenCalledOnce();
  } finally {finish();await cancelled;await resetting;}
  await new Promise(resolve=>setImmediate(resolve));
  expect(load).toHaveBeenCalledTimes(count);
  fixture.load=undefined;
  await expect(collect()).resolves.toEqual([]);
});

it("captures a background website without activation and bounds a stalled native capture",async()=>{
 const account=new WebsiteAccount("capture-owner"),tab=await account.open("about:blank");
 expect(await account.screenshot(tab.id)).toBe(Buffer.from("fixture-image").toString("base64"));
 expect(tab.view!.webContents.capturePage).toHaveBeenCalledWith(undefined,{stayHidden:true,stayAwake:true});
 vi.useFakeTimers();fixture.capture=()=>new Promise(()=>{});
 const rejected=expect(account.screenshot(tab.id)).rejects.toThrow("capture timed out");
 await vi.advanceTimersByTimeAsync(3001);await rejected;account.close();
});

it.each(["darwin","linux"] as const)("preserves minimized targets and restores presentation on %s without leaking window listeners",async(platform)=>{
 const window=new BrowserWindow(),account=new WebsiteAccount("minimize-owner",()=>{},platform);account.attach(window);
 const first=await account.open("about:blank"),second=await account.open("about:blank");
 const bounds={x:100,y:80,width:600,height:500},geometry=vi.spyOn(second.view!,"setBounds");
 account.present(second.id,bounds);expect(window.contentView.children.at(-1)).toBe(second.view);
 Reflect.set(window,"minimized",true);window.emit("minimize");expect(window.contentView.children).toHaveLength(platform==="darwin"?0:2);
 const third=await account.open("about:blank");account.present(second.id,bounds);
 expect(window.contentView.children).toHaveLength(platform==="darwin"?0:3);expect(second.view!.webContents.isDestroyed()).toBe(false);
 Reflect.set(window,"minimized",false);window.emit("restore");
 expect(window.contentView.children).toHaveLength(3);expect(window.contentView.children).toContain(first.view);expect(window.contentView.children).toContain(third.view);
 expect(window.contentView.children.at(-1)).toBe(second.view);expect(geometry).toHaveBeenLastCalledWith(bounds);
 account.close();expect(window.contentView.children).toEqual([]);expect(window.listenerCount("minimize")).toBe(0);expect(window.listenerCount("restore")).toBe(0);expect(window.listenerCount("closed")).toBe(0);
});

it("releases native window ownership even for an account that is no longer bound",async()=>{
 const window=new BrowserWindow(),account=new WebsiteAccount("unbound-owner");account.attach(window);await account.open("about:blank");
 window.emit("closed");expect(window.listenerCount("minimize")).toBe(0);expect(window.contentView.children).toEqual([]);
 await expect(account.open("about:blank")).resolves.toMatchObject({title:expect.any(Function)});account.close();
});

it("isolates Agent key accelerators only until their native dispatch settles",async()=>{
 const account=new WebsiteAccount("editing-owner"),tab=await account.open("about:blank"),contents=tab.view!.webContents;
 const shortcuts=vi.mocked(contents.setIgnoreMenuShortcuts);expect(shortcuts).not.toHaveBeenCalled();
 let finish!:(value:Record<string,unknown>)=>void;
 vi.spyOn(contents.debugger,"sendCommand").mockImplementationOnce(()=>new Promise(resolve=>{finish=resolve;}));
 const sending=tab.send("Input.dispatchKeyEvent",{type:"keyDown",key:"w",modifiers:4});
 expect(shortcuts).toHaveBeenLastCalledWith(true);
 // Cancellation can release a key while its original dispatch is still pending.
 await tab.send("Input.dispatchKeyEvent",{type:"keyUp",key:"w"});
 expect(shortcuts).toHaveBeenLastCalledWith(true);finish({});await sending;
 expect(shortcuts).toHaveBeenLastCalledWith(false);
 vi.spyOn(contents.debugger,"sendCommand").mockRejectedValueOnce(new Error("dispatch failed"));
 await expect(tab.send("Input.dispatchKeyEvent",{type:"keyUp",key:"w"})).rejects.toThrow("dispatch failed");
 expect(shortcuts).toHaveBeenLastCalledWith(false);account.close();
});
