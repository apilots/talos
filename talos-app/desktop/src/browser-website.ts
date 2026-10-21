import {createHash} from "node:crypto";
import {constants,mkdirSync,mkdtempSync,openSync,closeSync,fstatSync,fchmodSync,linkSync,rmSync} from "node:fs";
import path from "node:path";
import {fileURLToPath,pathToFileURL} from "node:url";
import {app,BaseWindow,WebContentsView,session,ipcMain,type IpcMainEvent,type DownloadItem,type Session} from "electron";
import {trackInputs,release,type Send} from "./browser-input";
import {BROWSER_CHROME_ACTION,BROWSER_CHROME_STATE,parseBrowserChromeAction,type BrowserChromeState} from "./browser-window-contract";
export type WebsiteTab={id:string;send:Send;view?:WebContentsView;url:()=>string;title:()=>string};
export const MAX_TABS=16;
const resources=path.join(path.dirname(fileURLToPath(import.meta.url)),"browser-window");
const chromeUrl=pathToFileURL(path.join(resources,"index.html")).href;
const homeUrl=pathToFileURL(path.join(resources,"home/home.html")).href;
const CHROME_HEIGHT=88;
export function checkedUrl(url:string):string {
  const parsed=new URL(url);
  if(url!=="about:blank" && (!["http:","https:"].includes(parsed.protocol)||parsed.username||parsed.password))throw new Error("Unsupported browser URL.");
  return parsed.href;
}
export function downloadRoot(user:string):string {
  const directory=path.join(app.getPath("userData"),"browser",createHash("sha256").update(user).digest("hex").slice(0,32),"downloads");
  mkdirSync(directory,{recursive:true,mode:0o700});return directory;
}
/** Website/profile owner. The trusted chrome is a sibling view, never an automation target. */
export class WebsiteAccount {
  private window:BaseWindow|undefined;
  private chrome:WebContentsView|undefined;
  private active:string|undefined;
  private readonly views=new Map<string,WebsiteTab>();
  private readonly partition:string;
  private error:string|null=null;
  private downloadSession:Session|undefined;
  private readonly downloads=new Map<DownloadItem,string>();
  readonly downloadDirectory:string;
  constructor(user:string) {this.downloadDirectory=downloadRoot(user);this.partition=`persist:talos-browser-${createHash("sha256").update(user).digest("hex").slice(0,32)}`;}
  private readonly onChromeAction=(event:IpcMainEvent,value:unknown)=>{
    const contents=this.chrome?.webContents;
    if(!contents || event.sender!==contents || event.senderFrame!==contents.mainFrame || contents.getURL()!==chromeUrl)return;
    void this.navigate(value).catch(error=>{console.error("Browser chrome navigation failed",error);this.error="The page could not be opened. Check the address and try again.";this.updateChrome();});
  };
  private readonly onDownload=(_event:Electron.Event,item:DownloadItem)=>{
    let directory:string|undefined;
    try {
      const filename=path.basename(item.getFilename());
      if(!filename||filename==="."||filename==="..")throw new Error("Invalid browser download filename.");
      // Only completed files are published to the receipt-visible download root.
      const pending=mkdtempSync(path.join(this.downloadDirectory,".pending-"));directory=pending;
      this.downloads.set(item,pending);
      item.once("done",(_doneEvent,state)=>{
        if(!this.downloads.delete(item))return;
        try {
          if(state==="completed")this.publishDownload(path.join(pending,filename),filename);
          else this.error="The download did not complete. Try downloading the file again.";
        } catch(error){this.failDownload(error);}
        finally {
          try {rmSync(pending,{recursive:true,force:true});}catch(error){this.failDownload(error);}
          this.updateChrome();
        }
      });
      item.on("updated",()=>{if(item.getReceivedBytes()>20*1024*1024)item.cancel();});
      item.setSavePath(path.join(pending,filename));this.error=null;this.updateChrome();
    } catch(error) {
      this.downloads.delete(item);item.cancel();
      if(directory)try {rmSync(directory,{recursive:true,force:true});}catch(cleanupError){this.failDownload(cleanupError);}
      this.failDownload(error);this.updateChrome();
    }
  };
  private failDownload(error:unknown):void {
    console.error("Browser download failed",error);
    this.error="The download could not be saved. Try downloading the file again.";
  }
  private publishDownload(source:string,filename:string):void {
    const descriptor=openSync(source,constants.O_RDONLY|(constants.O_NOFOLLOW??0));
    try {
      const stat=fstatSync(descriptor);
      if(!stat.isFile()||stat.size>20*1024*1024)throw new Error("Browser download exceeds its limit.");
      fchmodSync(descriptor,0o600);
    } finally {closeSync(descriptor);}
    const extension=path.extname(filename),stem=filename.slice(0,filename.length-extension.length);
    for(let index=0;index<1000;index++) {
      const candidate=path.join(this.downloadDirectory,index===0?filename:`${stem} (${index})${extension}`);
      // Both paths share the private account filesystem. Publish atomically without replacement.
      try {linkSync(source,candidate);return;}
      catch(error){if((error as NodeJS.ErrnoException).code==="EEXIST")continue;throw error;}
    }
    throw new Error("Browser download filename capacity exceeded.");
  }
  async ensure(check:()=>void=()=>{}):Promise<void> {
    check();
    if(!this.window || this.window.isDestroyed()) {
      this.window=new BaseWindow({width:1180,height:840,title:"Talos browser",show:true});
      this.window.on("resize",()=>this.resize());this.window.on("close",event=>{event.preventDefault();this.window?.hide();});
      this.chrome=new WebContentsView({webPreferences:{sandbox:true,contextIsolation:true,nodeIntegration:false,preload:path.join(resources,"preload.cjs")}});
      this.chrome.webContents.setWindowOpenHandler(()=>({action:"deny"}));
      this.chrome.webContents.on("will-navigate",event=>event.preventDefault());
      this.chrome.webContents.on("did-finish-load",()=>this.updateChrome());
      ipcMain.on(BROWSER_CHROME_ACTION,this.onChromeAction);
      this.window.contentView.addChildView(this.chrome);this.resize();
      await this.chrome.webContents.loadURL(chromeUrl);check();
    }
    if(!this.views.size)await this.open("about:blank",check);
  }
  show():void {this.window?.show();this.window?.focus();}
  async tabs(check:()=>void=()=>{}):Promise<WebsiteTab[]> {await this.ensure(check);check();return [...this.views.values()];}
  async open(url:string,check:()=>void=()=>{}):Promise<WebsiteTab> {
    check();
    if(this.views.size>=MAX_TABS)throw new Error("Browser tab capacity exceeded.");
    if(!this.window || this.window.isDestroyed()){await this.ensure(check);check();if(url==="about:blank")return [...this.views.values()][0];}
    const browserSession=session.fromPartition(this.partition);
    if(this.downloadSession!==browserSession) {
      this.downloadSession?.removeListener("will-download",this.onDownload);
      browserSession.on("will-download",this.onDownload);this.downloadSession=browserSession;
    }
    browserSession.setPermissionRequestHandler((_contents,_permission,callback)=>callback(false));
    const view=new WebContentsView({webPreferences:{partition:this.partition,sandbox:true,contextIsolation:true,nodeIntegration:false}});
    const wc=view.webContents,id=String(wc.id);wc.debugger.attach("1.3");
    const tab:WebsiteTab={id,view,send:trackInputs((method,params)=>wc.debugger.sendCommand(method,params)),url:()=>wc.getURL()===homeUrl?"about:blank":wc.getURL(),title:()=>wc.getTitle()};
    this.views.set(id,tab);
    wc.once("destroyed",()=>{this.views.delete(id);this.updateChrome();});
    wc.on("did-navigate",()=>this.updateChrome());
    wc.on("did-navigate-in-page",()=>this.updateChrome());
    wc.on("did-start-loading",()=>this.updateChrome());
    wc.on("did-stop-loading",()=>this.updateChrome());
    wc.on("page-title-updated",()=>this.updateChrome());
    wc.on("did-fail-load",(_event,code)=>{if(code!==-3){this.error="This page could not be loaded. Check your connection or try another address.";this.updateChrome();}});
    wc.on("before-input-event",(event,input)=>{
      if(input.type!=="keyDown" || !(input.control || input.meta) || input.alt)return;
      if(input.key.toLowerCase()==="l"){event.preventDefault();this.chrome?.webContents.focus();this.chrome?.webContents.send(BROWSER_CHROME_STATE,{focusAddress:true});}
    });
    wc.setWindowOpenHandler(({url:target})=>{try{checkedUrl(target);void this.open(target).catch(error=>console.error("Website popup failed",error));}catch{/* Unsupported website popups are denied. */}return {action:"deny"};});
    const navigation=(event:Electron.Event,target:string)=>{try{if(target!==homeUrl)checkedUrl(target);}catch{event.preventDefault();}};
    wc.on("will-navigate",navigation);wc.on("will-redirect",navigation);
    check();await this.focus(id);check();await wc.loadURL(url==="about:blank"?homeUrl:checkedUrl(url));check();this.updateChrome();return tab;
  }
  async focus(id:string):Promise<void> {
    const tab=this.views.get(id);if(!tab?.view || !this.window)throw new Error("Unknown tab.");
    for(const other of this.views.values())if(other.view)this.window.contentView.removeChildView(other.view);
    this.window.contentView.addChildView(tab.view);this.active=id;this.resize();tab.view.webContents.focus();this.updateChrome();
  }
  private resize():void {
    if(!this.window)return;const [width,height]=this.window.getContentSize();
    this.chrome?.setBounds({x:0,y:0,width,height:CHROME_HEIGHT});
    for(const tab of this.views.values())tab.view?.setBounds({x:0,y:CHROME_HEIGHT,width,height:Math.max(0,height-CHROME_HEIGHT)});
  }
  private updateChrome():void {
    if(!this.chrome || this.chrome.webContents.isDestroyed())return;
    const active=this.views.get(this.active??""),wc=active?.view?.webContents;
    const state:BrowserChromeState={tabs:[...this.views.values()].map(tab=>({id:tab.id,title:tab.title().slice(0,200)||"New tab",active:tab.id===this.active})),address:active?.url()==="about:blank"?"":(active?.url()??"").slice(0,8192),back:wc?.navigationHistory.canGoBack()??false,forward:wc?.navigationHistory.canGoForward()??false,loading:wc?.isLoading()??false,error:this.error};
    this.window?.setTitle(`Talos browser${active?.title()?` — ${active.title().slice(0,180)}`:""}`);
    this.chrome.webContents.send(BROWSER_CHROME_STATE,state);
  }
  private async navigate(value:unknown):Promise<void> {
    const action=parseBrowserChromeAction(value);this.error=null;
    const wc=this.views.get(this.active??"")?.view?.webContents;
    switch(action.kind) {
      case "state":break;
      case "navigate":if(!wc)throw new Error("No browser tab.");await wc.loadURL(checkedUrl(action.url.includes("://")?action.url:`https://${action.url}`));break;
      case "back":if(wc?.navigationHistory.canGoBack())wc.navigationHistory.goBack();break;
      case "forward":if(wc?.navigationHistory.canGoForward())wc.navigationHistory.goForward();break;
      case "reload":wc?.reload();break;
      case "new":await this.open("about:blank");break;
      case "focus":await this.focus(action.tabId);break;
      case "close":await this.remove(action.tabId);if(!this.views.size)await this.open("about:blank");break;
      case "hide":this.window?.hide();break;
    }
    this.updateChrome();
  }
  async remove(id:string):Promise<void> {
    const tab=this.views.get(id);if(tab?.view){this.window?.contentView.removeChildView(tab.view);tab.view.webContents.close({waitForBeforeUnload:false});this.views.delete(id);}
    const next=this.active && this.views.has(this.active) ? this.active : this.views.keys().next().value;
    if(next && this.active!==next)await this.focus(next);else {this.active=next;this.updateChrome();}
  }
  async releaseInputs():Promise<void> {for(const tab of this.views.values())await release(tab.send);}
  close():void {
    this.downloadSession?.removeListener("will-download",this.onDownload);this.downloadSession=undefined;
    for(const [item,directory] of this.downloads) {
      this.downloads.delete(item);item.cancel();
      try {rmSync(directory,{recursive:true,force:true});}catch(error){this.failDownload(error);}
    }
    ipcMain.removeListener(BROWSER_CHROME_ACTION,this.onChromeAction);
    for(const tab of this.views.values())tab.view?.webContents.close({waitForBeforeUnload:false});this.views.clear();
    this.chrome?.webContents.close({waitForBeforeUnload:false});this.chrome=undefined;
    this.window?.removeAllListeners("close");this.window?.close();this.window=undefined;
  }
}
