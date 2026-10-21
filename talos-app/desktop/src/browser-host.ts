import { createHash } from "node:crypto";
import path from "node:path";
import { homedir } from "node:os";
import { constants, mkdirSync, mkdtempSync, openSync, closeSync, readFileSync, readdirSync, fstatSync, realpathSync, writeFileSync, rmSync } from "node:fs";
import { WebsiteAccount, checkedUrl, downloadRoot, MAX_TABS, type WebsiteTab as Tab } from "./browser-website";
import { trackInputs, release, type Send, type RecordValue } from "./browser-input";
import type { AppProtocol as P } from "@talos/sdk-core";

type Operation = { cancelled: boolean; user: string; generation: string; epoch: number };
const keyCodes: Record<string, number> = { Enter:13, Tab:9, Escape:27, Backspace:8, Delete:46, ArrowLeft:37, ArrowUp:38, ArrowRight:39, ArrowDown:40, Home:36, End:35, PageUp:33, PageDown:34, Space:32 };

/** Only dedicated website views are attached. The product renderer is never a target. */
export class DesktopBrowserHost {
  private readonly operations = new Map<string, Operation>();
  private readonly accounts = new Map<string, WebsiteAccount>();
  private chrome: ChromeConnection | undefined;
  private chromeOwner: string | undefined;
  private chromeRevision = 0;
  private connecting: Promise<void> | undefined;
  private generation: string | undefined;
  private resetting:Promise<void>|undefined;

  cancel(id: string): void { const call=this.operations.get(id); if (call) call.cancelled=true; }
  reset():Promise<void> {
    if(this.resetting)return this.resetting;
    const pending=this.resetOwnedTargets().finally(()=>{if(this.resetting===pending)this.resetting=undefined;});
    this.resetting=pending;return pending;
  }
  private async resetOwnedTargets(): Promise<void> {
    this.chromeRevision++;
    for (const call of this.operations.values()) call.cancelled=true;
    this.operations.clear();
    try {await this.chrome?.releaseInputs();}catch(error){console.error("Chrome input cleanup failed",error);}finally{this.chrome?.close();this.chrome=undefined;this.chromeOwner=undefined;}
    for (const account of this.accounts.values()) account.close();
    this.accounts.clear(); this.generation=undefined;
  }
  async execute(id:string, request:P.BrowserHostRequest): Promise<P.BrowserHostResponse> {
    if(this.resetting)await this.resetting;
    if (!request.userId || !request.generation || !Number.isSafeInteger(request.epoch) || this.operations.size>=8) throw new Error("Invalid browser host context.");
    if (this.generation && this.generation!==request.generation) await this.reset();
    this.generation=request.generation;
    const op:Operation={cancelled:false,user:request.userId,generation:request.generation,epoch:request.epoch};
    this.operations.set(id,op);
    const check=()=> { if (op.cancelled || this.generation!==op.generation) throw new Error("Browser operation cancelled."); };
    try {
      const operation=request.operation;
      if(operation.kind==="probe")return {kind:"readiness",connected:this.chromeOwner===request.userId&&Boolean(this.chrome?.isConnected())};
      if (operation.kind==="releaseInputs" || operation.kind==="shutdown") {
        for (const [other,call] of this.operations) if (other!==id) call.cancelled=true;
        for (const account of this.accounts.values()) await account.releaseInputs();
        await this.chrome?.releaseInputs();
        if (operation.kind==="shutdown") await this.reset();
        return {kind:"done"};
      }
      if (operation.kind==="connect") {
        const previous = this.connecting;
        const revision = ++this.chromeRevision;
        // One fixed Chrome owns global download behavior. Retire an older
        // candidate before the next handshake can configure that same browser.
        const pending = (async () => {
          if (previous) await previous.catch(() => {});
          check();
          if (revision !== this.chromeRevision) throw new Error("Browser connection superseded.");
          this.chrome?.close(); this.chrome=undefined; this.chromeOwner=undefined;
          const candidate = await ChromeConnection.connect(request.userId);
          try {
            check();
            if (revision !== this.chromeRevision) throw new Error("Browser connection superseded.");
            this.chrome = candidate;
            this.chromeOwner = request.userId;
          } catch (error) {
            candidate.close();
            throw error;
          }
        })();
        this.connecting = pending;
        try { await pending; }
        finally { if (this.connecting === pending) this.connecting = undefined; }
        return {kind:"done"};
      }
      if (operation.kind==="disconnect") {
        this.chromeRevision++;
        if (this.chromeOwner!==request.userId) throw new Error("Chrome belongs to another account.");
        try {await this.chrome?.releaseInputs();}finally{this.chrome?.close();this.chrome=undefined;this.chromeOwner=undefined;}return {kind:"done"};
      }
      let account=this.accounts.get(request.userId);
      if (!account) { if(this.accounts.size>=2) throw new Error("Browser account capacity exceeded."); account=new WebsiteAccount(request.userId); this.accounts.set(request.userId,account); }
      if (operation.kind==="show") {await account.ensure();check();account.show();return {kind:"done"};}
      if(operation.kind==="acknowledgeDownloads") {
        if(operation.files.length>5)throw new Error("Too many browser file receipts.");
        if(request.target==="connected"&&(!this.chrome||this.chromeOwner!==request.userId))throw new Error("The connected browser owner changed.");
        const directory=request.target==="connected" ? this.chrome!.downloadDirectory : account.downloadDirectory;
        for(const file of operation.files) {
          if(path.basename(file.filename)!==file.filename||!/^[a-f0-9]{64}$/.test(file.sha256))throw new Error("Invalid browser file receipt.");
          const candidate=(request.target==="connected" ? this.chrome!.collectDownloads() : completedDownloads(directory)).find(entry=>entry.filename===file.filename);
          if(candidate && createHash("sha256").update(Buffer.from(candidate.dataBase64,"base64")).digest("hex")===file.sha256)rmSync(path.join(directory,file.filename));
        }
        return {kind:"done"};
      }
      if (operation.kind!=="execute") throw new Error("Unsupported browser operation.");
      const connected=request.target==="connected";
      if (connected && (!this.chrome || this.chromeOwner!==request.userId)) throw new Error("Connect your prepared Chrome profile in Browser settings first.");
      const owner=connected ? this.chrome! : account;
      let tabs=await owner.tabs();check();
      let tab:Tab|undefined=operation.targetId ? tabs.find(t=>t.id===operation.targetId) : tabs[0];
      const action=operation.action;
      if (operation.targetId && !tab && action.kind!=="state" && action.kind!=="tab") {
        throw new Error("The browser tab was closed. Inspect the page again.");
      }
      if (!tab && action.kind==="state") tab=tabs[0];
      if (action.kind==="tab") {
        if (action.action==="open") { tab=await owner.open(checkedUrl(action.url ?? "about:blank"),check);check(); }
        else {
          const tabId=action.tabId ?? (action.action==="close" ? operation.targetId : null);
          tab=tabs.find(t=>t.id===tabId);
          if(!tab) throw new Error("Unknown browser tab.");
          if (action.action==="close") {check();await owner.remove(tab.id);const remaining=await owner.tabs();tab=remaining.find(value=>value.id===operation.targetId)??remaining[0];}
          else {check();await owner.focus(tab.id);}
        }
      }
      if (!tab) {check();tab=await owner.open("about:blank",check);}
      const current=tab;
      const send:Send=async(method,params)=>{check(); const result=await current.send(method,params);check();return result;};
      const prepareInput=async()=>{
        await send("Input.setIgnoreInputEvents",{ignore:false});
        await send("Emulation.setFocusEmulationEnabled",{enabled:true});
        await send("Page.bringToFront");
      };
      await send("Page.enable");
      if (action.kind==="navigate") {
        await send("Page.navigate",{url:checkedUrl(action.url)});
        // Observe the navigation loader before returning a document revision.
        for(let attempt=0;attempt<50;attempt++) {const ready=await send("Runtime.evaluate",{expression:"document.readyState",returnByValue:true});if(object(ready.result).value==="complete")break;await new Promise(resolve=>setTimeout(resolve,50));check();}
      }
      const frameTree=object((await send("Page.getFrameTree")).frameTree);
      const frame=object(frameTree.frame); const loaderId=String(frame.loaderId ?? "");
      if (action.kind==="click" || action.kind==="fill" || action.kind==="upload") {
        const element=operation.element;
        if(!element || element.targetId!==current.id || element.loaderId!==loaderId) throw new Error("Stale browser element. Inspect the page again.");
        const described=object((await send("DOM.describeNode",{backendNodeId:element.backendNodeId})).node);
        const currentAttributes=described.attributes as string[]??[];const attributes=Object.fromEntries(Array.from({length:currentAttributes.length/2},(_,index)=>[currentAttributes[index*2],currentAttributes[index*2+1]]));
        if(String(described.nodeName).toLowerCase()!==element.tag || Object.entries(element.attributes).some(([key,value])=>attributes[key]!==value)) throw new Error("Stale browser element.");
        if(action.kind==="click") {
          await prepareInput();
          const quads=(await send("DOM.getContentQuads",{backendNodeId:element.backendNodeId})).quads as number[][];
          const q=quads?.[0];if(!q||q.length!==8)throw new Error("Element is not visible.");
          const x=(q[0]+q[2]+q[4]+q[6])/4,y=(q[1]+q[3]+q[5]+q[7])/4;
          await send("Input.dispatchMouseEvent",{type:"mouseMoved",x,y});
          await send("Input.dispatchMouseEvent",{type:"mousePressed",x,y,button:"left",buttons:1,clickCount:1});
          await send("Input.dispatchMouseEvent",{type:"mouseReleased",x,y,button:"left",buttons:0,clickCount:1});
          await send("Runtime.evaluate",{expression:"new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve(0))))",awaitPromise:true,returnByValue:true});
        } else if(action.kind==="fill") {
          if(!["input","textarea"].includes(element.tag)&&element.attributes.contenteditable!=="true")throw new Error("Select a text input or editable element.");
          await prepareInput();
          await send("DOM.focus",{backendNodeId:element.backendNodeId});
          const modifier=process.platform==="darwin"?4:2;
          await send("Input.dispatchKeyEvent",{type:"keyDown",key:"a",code:"KeyA",windowsVirtualKeyCode:65,modifiers:modifier});
          await send("Input.dispatchKeyEvent",{type:"keyUp",key:"a",code:"KeyA",windowsVirtualKeyCode:65,modifiers:modifier});
          await send("Input.insertText",{text:action.text});
        } else {
          const upload=operation.upload;
          if(!upload || upload.dataBase64.length>28*1024*1024)throw new Error("An authorized Turn attachment is required.");
          const directory=mkdtempSync(path.join(owner.downloadDirectory,".upload-"));
          try {
            const filename=path.basename(upload.filename);if(!filename || filename==="." || filename==="..")throw new Error("Invalid attachment filename.");
            const filePath=path.join(directory,filename), bytes=Buffer.from(upload.dataBase64,"base64");if(bytes.length>20*1024*1024)throw new Error("Attachment too large.");
            writeFileSync(filePath,bytes,{mode:0o600,flag:"wx"});
            await send("DOM.setFileInputFiles",{backendNodeId:element.backendNodeId,files:[filePath]});
          } finally {rmSync(directory,{recursive:true,force:true});}
        }
      } else if(action.kind==="key") {
        await prepareInput();
        const parts=action.key.split("+");const key=parts.pop()!;
        const aliases:Record<string,number>={alt:1,control:2,ctrl:2,meta:4,command:4,cmd:4,shift:8};
        if(parts.some(part=>aliases[part.toLowerCase()]===undefined))throw new Error("Unsupported key modifier.");
        const modifiers=parts.reduce((mask,part)=>mask|aliases[part.toLowerCase()],0);
        if(!keyCodes[key] && key.length!==1) throw new Error("Unsupported key.");
        const params={key:key==="Space"?" ":key,windowsVirtualKeyCode:keyCodes[key]??key.toUpperCase().charCodeAt(0),modifiers};
        await send("Input.dispatchKeyEvent",{...params,type:"keyDown"});await send("Input.dispatchKeyEvent",{...params,type:"keyUp"});
      } else if(action.kind==="scroll") {
        await prepareInput();
        await send("Input.dispatchMouseEvent",{type:"mouseWheel",x:200,y:200,deltaX:action.deltaX,deltaY:action.deltaY});
      }
      const downloads=action.kind==="download" ? (connected ? this.chrome!.collectDownloads() : completedDownloads(owner.downloadDirectory)) : [];
      const elements:P.BrowserHostElement[]=[];
      if(action.kind==="state") {
        const doc=object((await send("DOM.getDocument",{depth:1})).root);
        const selected=(await send("DOM.querySelectorAll",{nodeId:doc.nodeId,selector:"a,button,input,textarea,select,[role=button],[contenteditable=true]"})).nodeIds as number[];
        for(const nodeId of selected.slice(0,150)) {
          const node=object((await send("DOM.describeNode",{nodeId})).node), attrs=node.attributes as string[] ?? [], attributes:Record<string,string>={};
          for(let i=0;i<attrs.length;i+=2) if(["id","name","type","aria-label","placeholder","href","role","value","contenteditable"].includes(attrs[i]))attributes[attrs[i]]=attrs[i+1].slice(0,512);
          elements.push({backendNodeId:Number(node.backendNodeId),tag:String(node.nodeName).toLowerCase(),attributes,targetId:current.id,loaderId});
        }
      }
      const textResult=object((await send("Runtime.evaluate",{expression:"(document.body?.innerText ?? '').slice(0,16000)",returnByValue:true})).result);
      const titleResult=object((await send("Runtime.evaluate",{expression:"document.title.slice(0,512)",returnByValue:true})).result);
      const screenshot=action.kind==="screenshot" ? String((await send("Page.captureScreenshot",{format:"png",captureBeyondViewport:false})).data) : null;
      if(screenshot && screenshot.length>6*1024*1024)throw new Error("Browser screenshot is too large.");
      tabs=await owner.tabs();check();
      return {kind:"observation",page:{targetId:current.id,url:current.url()==="about:blank"?"about:blank":String(frame.url??current.url()),title:String(titleResult.value??current.title()),text:String(textResult.value??""),tabs:tabs.slice(0,32).map(t=>({tabId:t.id,title:t.title(),url:t.url()}))},loaderId,elements,screenshot,downloads};
    } finally { this.operations.delete(id); }
  }
}
function object(value:unknown):RecordValue {return typeof value==="object"&&value!==null ? value as RecordValue : {};}


/** Explicitly authorized, fixed loopback Chrome; never scans profiles or remote endpoints. */
class ChromeConnection {
  private nextId=0;
  private readonly pending=new Map<number,{resolve:(value:RecordValue)=>void;reject:(error:Error)=>void;timer:ReturnType<typeof setTimeout>}>();
  private readonly sessions=new Map<string,string>();
  private readonly senders=new Map<string,Send>();
  readonly downloadDirectory:string;
  private readonly downloadSourceDirectory:string;
  private readonly downloads=new Map<string,{filename:string;completed:boolean;finalFilename?:string}>();
  private constructor(private readonly socket:WebSocket,user:string) {
    this.downloadDirectory=downloadRoot(user+"-chrome");
    // CDP GUID files are private sources, including across disconnects and restarts.
    this.downloadSourceDirectory=path.join(this.downloadDirectory,".chrome-pending");
    mkdirSync(this.downloadSourceDirectory,{recursive:true,mode:0o700});
    socket.addEventListener("message",event=>{if(String(event.data).length>40*1024*1024){this.close();return;}let decoded:unknown;try{decoded=JSON.parse(String(event.data));}catch{this.close();return;}const message=object(decoded) as {id?:number;method?:string;params?:RecordValue;result?:RecordValue;error?:unknown};
      if(message.method==="Browser.downloadWillBegin" && message.params) {const guid=String(message.params.guid);if(/^[a-z0-9-]{1,80}$/i.test(guid))this.downloads.set(guid,{filename:path.basename(String(message.params.suggestedFilename)),completed:false});}
      if(message.method==="Browser.downloadProgress" && message.params?.state==="completed") {
        const guid=String(message.params.guid),download=this.downloads.get(guid);
        if(download){download.completed=true;try{this.finalizeDownload(guid,download);}catch(error){console.error("Chrome download finalization failed",error);}}
      }
      if(message.id===undefined)return;const pending=this.pending.get(message.id);if(!pending)return;clearTimeout(pending.timer);this.pending.delete(message.id);if(message.error)pending.reject(new Error("Chrome operation rejected."));else pending.resolve(message.result??{});});
    socket.addEventListener("close",()=>this.fail());socket.addEventListener("error",()=>this.fail());
  }
  static async connect(user:string):Promise<ChromeConnection> {
    const response=await fetch("http://127.0.0.1:9222/json/version",{signal:AbortSignal.timeout(3000),redirect:"error"});
    const version=await response.json() as {Browser:string;webSocketDebuggerUrl:string};
    if(!/^(?:Chrome|Chromium)\/(\d+)/.test(version.Browser)||Number(version.Browser.match(/\/(\d+)/)?.[1])<136)throw new Error("A supported Chrome 136+ debug profile is required.");
    const endpoint=new URL(version.webSocketDebuggerUrl);
    if(endpoint.protocol!=="ws:"||!["127.0.0.1","localhost"].includes(endpoint.hostname)||endpoint.port!=="9222"||endpoint.username||endpoint.password||endpoint.search||!endpoint.pathname.startsWith("/devtools/browser/"))throw new Error("Chrome endpoint is not local.");
    const socket=new WebSocket(endpoint);await new Promise<void>((resolve,reject)=>{const timer=setTimeout(()=>{socket.close();reject(new Error("Chrome connection timed out."));},3000);socket.addEventListener("open",()=>{clearTimeout(timer);resolve();},{once:true});socket.addEventListener("error",()=>{clearTimeout(timer);reject(new Error("Chrome connection failed."));},{once:true});});
    const client=new ChromeConnection(socket,user);
    try {
      const args=(await client.send("Browser.getBrowserCommandLine")).arguments as string[];
      const profile=args.find(arg=>arg.startsWith("--user-data-dir="))?.slice("--user-data-dir=".length);
      const defaults=[...['Google/Chrome','Google/Chrome Beta','Google/Chrome Canary','Google/Chrome Dev','Chromium'].map(name=>path.join(homedir(),'Library/Application Support',name)),...['google-chrome','google-chrome-beta','google-chrome-unstable','chromium'].map(name=>path.join(homedir(),'.config',name)),...['Google/Chrome','Google/Chrome Beta','Google/Chrome SxS','Chromium'].map(name=>path.join(process.env.LOCALAPPDATA??homedir(),name,'User Data'))];
      const resolvedProfile=profile&&path.isAbsolute(profile)?realpathSync(profile):null;
      if(!resolvedProfile||defaults.some(dir=>{let resolved=path.resolve(dir);try{resolved=realpathSync(dir);}catch{/* An absent default profile still has a reserved path. */}const candidate=process.platform==="linux"?resolvedProfile:resolvedProfile.toLowerCase(),reserved=process.platform==="linux"?resolved:resolved.toLowerCase();return candidate===reserved||candidate.startsWith(reserved+path.sep);}))throw new Error("Use an explicit non-default Chrome profile with --enable-automation.");
      await client.send("Browser.setDownloadBehavior",{behavior:"allowAndName",downloadPath:client.downloadSourceDirectory,eventsEnabled:true});
      return client;
    } catch(error) {client.close();throw error;}
  }
  private finalizeDownload(guid:string,download:{filename:string;completed:boolean;finalFilename?:string}):void {
    const source=path.join(this.downloadSourceDirectory,guid);
    if(!download.finalFilename) {
      const descriptor=openSync(source,constants.O_RDONLY|(constants.O_NOFOLLOW??0));
      let bytes:Buffer;
      try {
        const stat=fstatSync(descriptor);
        if(!stat.isFile()||stat.size>20*1024*1024)throw new Error("Chrome download exceeds its limit.");
        bytes=readFileSync(descriptor);
        if(bytes.length>20*1024*1024)throw new Error("Chrome download exceeds its limit.");
      } finally {closeSync(descriptor);}
      const filename=download.filename;
      if(!filename||filename==="."||filename==="..")throw new Error("Invalid Chrome download filename.");
      const extension=path.extname(filename),stem=filename.slice(0,filename.length-extension.length);
      for(let index=0;index<1000;index++) {
        const candidate=index===0?filename:`${stem} (${index})${extension}`;
        let output:number;
        try {output=openSync(path.join(this.downloadDirectory,candidate),"wx",0o600);}
        catch(error){if((error as NodeJS.ErrnoException).code==="EEXIST")continue;throw error;}
        try {writeFileSync(output,bytes);download.finalFilename=candidate;}
        catch(error){rmSync(path.join(this.downloadDirectory,candidate),{force:true});throw error;}
        finally {closeSync(output);}
        break;
      }
      if(!download.finalFilename)throw new Error("Chrome download filename capacity exceeded.");
    }
    // Keep the assigned output when source cleanup fails, so retry cannot create duplicates.
    rmSync(source,{force:true});
    this.downloads.delete(guid);
  }
  collectDownloads():P.BrowserHostFile[] {
    for(const [guid,download] of this.downloads)if(download.completed) {
      try {this.finalizeDownload(guid,download);}
      catch(error){throw new Error("A completed Chrome download could not be prepared. Retry collecting downloads.",{cause:error});}
    }
    return completedDownloads(this.downloadDirectory);
  }
  isConnected():boolean {return this.socket.readyState===WebSocket.OPEN;}
  private fail():void {for(const p of this.pending.values()){clearTimeout(p.timer);p.reject(new Error("Chrome disconnected."));}this.pending.clear();}
  async send(method:string,params:RecordValue={},sessionId?:string):Promise<RecordValue> {
    if(this.socket.readyState!==WebSocket.OPEN||this.pending.size>=32)throw new Error("Chrome connection is unavailable.");
    const id=++this.nextId;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{this.pending.delete(id);reject(new Error("Chrome command timed out."));},3000);this.pending.set(id,{resolve,reject,timer});this.socket.send(JSON.stringify({id,method,params,...(sessionId?{sessionId}:{})}));});
  }
  async tabs():Promise<Tab[]> {
    const targets=(await this.send("Target.getTargets")).targetInfos as {targetId:string;type:string;url:string;title:string}[];
    return Promise.all(targets.filter(t=>t.type==="page"&&(t.url==="about:blank"||/^https?:/.test(t.url))).slice(0,32).map(async t=>{let id=this.sessions.get(t.targetId);if(!id){id=String((await this.send("Target.attachToTarget",{targetId:t.targetId,flatten:true})).sessionId);this.sessions.set(t.targetId,id);}let sender=this.senders.get(t.targetId);if(!sender){sender=trackInputs((method:string,params?:RecordValue)=>this.send(method,params,id));this.senders.set(t.targetId,sender);}return {id:t.targetId,send:sender,url:()=>t.url,title:()=>t.title};}));
  }
  async open(url:string,check:()=>void=()=>{}):Promise<Tab> {if((await this.tabs()).length>=MAX_TABS)throw new Error("Chrome tab capacity exceeded.");check();const id=String((await this.send("Target.createTarget",{url})).targetId);const tab=(await this.tabs()).find(t=>t.id===id);if(!tab)throw new Error("Chrome tab was not created.");return tab;}
  async remove(id:string):Promise<void> {
    await this.send("Target.closeTarget",{targetId:id});
    // Close acknowledgement can precede target destruction. Do not reattach a
    // still-listed closing target while choosing the next observation.
    const deadline=Date.now()+3000;
    do {
      const targets=(await this.send("Target.getTargets")).targetInfos as {targetId:string}[];
      if(!targets.some(target=>target.targetId===id)) {
        this.sessions.delete(id);this.senders.delete(id);return;
      }
      await new Promise(resolve=>setTimeout(resolve,50));
    } while(Date.now()<deadline);
    throw new Error("Chrome tab close timed out.");
  }
  async focus(id:string):Promise<void> {await this.send("Target.activateTarget",{targetId:id});}
  async releaseInputs():Promise<void> {for(const tab of await this.tabs())await release(tab.send);}
  close():void {if(this.socket.readyState===WebSocket.OPEN)for(const sessionId of this.sessions.values())this.socket.send(JSON.stringify({id:++this.nextId,sessionId,method:"Emulation.setFocusEmulationEnabled",params:{enabled:false}}));if(this.socket.readyState===WebSocket.OPEN)this.socket.send(JSON.stringify({id:++this.nextId,method:"Browser.setDownloadBehavior",params:{behavior:"default"}}));this.fail();this.socket.close();}
}

export const desktopBrowserHost = new DesktopBrowserHost();

function completedDownloads(directory:string):P.BrowserHostFile[] {
  const files:P.BrowserHostFile[]=[];let total=0;
  for(const item of readdirSync(directory,{withFileTypes:true})) {
    if(!item.isFile() || item.name.startsWith(".") || item.name.endsWith(".crdownload"))continue;
    const descriptor=openSync(path.join(directory,item.name),constants.O_RDONLY | (constants.O_NOFOLLOW??0));
    try {const stat=fstatSync(descriptor);total+=stat.size;if(!stat.isFile())continue;if(total>20*1024*1024||files.length>=5)throw new Error("Collect up to five downloads totalling 20 MiB.");files.push({filename:item.name,dataBase64:readFileSync(descriptor).toString("base64")});}finally{closeSync(descriptor);}
  }
  return files;
}
