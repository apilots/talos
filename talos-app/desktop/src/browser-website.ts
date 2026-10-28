import {createHash} from "node:crypto";
import {constants,mkdirSync,mkdtempSync,openSync,closeSync,fstatSync,fchmodSync,linkSync,rmSync} from "node:fs";
import path from "node:path";
import {fileURLToPath,pathToFileURL} from "node:url";
import {app,WebContentsView,session,type BaseWindow,type DownloadItem,type Session} from "electron";
import {trackInputs,release,type Send} from "./browser-input";
import type {DesktopBrowserBounds,DesktopBrowserTab} from "@talos/sdk-app-server/desktop";
export type WebsiteTab={id:string;send:Send;view?:WebContentsView;url:()=>string;title:()=>string};
export const MAX_TABS=16;
const resources=path.join(path.dirname(fileURLToPath(import.meta.url)),"browser-window");
const homeUrl=pathToFileURL(path.join(resources,"home/home.html")).href;
export function checkedUrl(url:string):string {
  const parsed=new URL(url);
  if(url!=="about:blank" && (!["http:","https:"].includes(parsed.protocol)||parsed.username||parsed.password))throw new Error("Unsupported browser URL.");
  return parsed.href;
}
export function downloadRoot(user:string):string {
  const directory=path.join(app.getPath("userData"),"browser",createHash("sha256").update(user).digest("hex").slice(0,32),"downloads");
  mkdirSync(directory,{recursive:true,mode:0o700});return directory;
}
export interface WebsitePreviewOptions {
  partition: string;
  onPopup: (url: string) => void;
}
/** Owns isolated website targets. Presentation never changes the operation target. */
export class WebsiteAccount {
  private window:BaseWindow|undefined;
  private presentation:{id:string;bounds:DesktopBrowserBounds}|undefined;
  private get detachedWhileMinimized():boolean {return this.platform==="darwin" && Boolean(this.window?.isMinimized());}
  private readonly windowClosed=()=>this.attach(undefined);
  private readonly synchronizeViews=()=>{
    const window=this.window;if(!window || window.isDestroyed())return;
    for(const tab of this.views.values())if(tab.view) {
      // A minimized macOS native ancestor suspends its compositor even with
      // backgroundThrottling disabled. Detached targets keep painting without
      // creating or restoring an OS window. Reattach the same views on restore.
      if(this.detachedWhileMinimized)window.contentView.removeChildView(tab.view);
      else if(this.presentation?.id===tab.id) {tab.view.setBounds(this.presentation.bounds);window.contentView.addChildView(tab.view);}
      else window.contentView.addChildView(tab.view,0);
    }
  };
  private readonly views=new Map<string,WebsiteTab>();
  private readonly partition:string;
  error:string|null=null;
  private downloadSession:Session|undefined;
  private readonly downloads=new Map<DownloadItem,string>();
  readonly downloadDirectory:string;
  constructor(user:string,private readonly changed:()=>void=()=>{},private readonly platform:NodeJS.Platform=process.platform,private readonly preview?:WebsitePreviewOptions) {this.downloadDirectory=downloadRoot(user);this.partition=preview?.partition??`persist:talos-browser-${createHash("sha256").update(user).digest("hex").slice(0,32)}`;}
  private checkedUrl(url:string):string {
    if(this.preview && new URL(url).protocol==="talos-preview:")return new URL(url).href;
    return checkedUrl(url);
  }
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
          this.changed();
        }
      });
      item.on("updated",()=>{if(item.getReceivedBytes()>20*1024*1024)item.cancel();});
      item.setSavePath(path.join(pending,filename));this.error=null;this.changed();
    } catch(error) {
      this.downloads.delete(item);item.cancel();
      if(directory)try {rmSync(directory,{recursive:true,force:true});}catch(cleanupError){this.failDownload(cleanupError);}
      this.failDownload(error);this.changed();
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
  async ensure(check:()=>void=()=>{}):Promise<void> {check();if(!this.views.size)await this.open("about:blank",check);}
  async tabs(check:()=>void=()=>{}):Promise<WebsiteTab[]> {await this.ensure(check);check();return [...this.views.values()];}
  snapshot():DesktopBrowserTab[] {
    return [...this.views.values()].map(tab=>({id:tab.id,title:tab.title()||"New tab",url:tab.url(),back:tab.view?.webContents.navigationHistory.canGoBack()??false,forward:tab.view?.webContents.navigationHistory.canGoForward()??false,loading:tab.view?.webContents.isLoading()??false}));
  }
  async open(url:string,check:()=>void=()=>{}):Promise<WebsiteTab> {
    check();if(this.views.size>=MAX_TABS)throw new Error("Browser tab capacity exceeded.");
    this.checkedUrl(url);
    const browserSession=session.fromPartition(this.partition);
    if(this.downloadSession!==browserSession) {
      this.downloadSession?.removeListener("will-download",this.onDownload);
      browserSession.on("will-download",this.onDownload);this.downloadSession=browserSession;
    }
    browserSession.setPermissionRequestHandler((_contents,_permission,callback)=>callback(false));
    const view=new WebContentsView({webPreferences:{partition:this.partition,sandbox:true,contextIsolation:true,nodeIntegration:false,backgroundThrottling:false,focusOnNavigation:false,disableDialogs:true}});
    const wc=view.webContents,id=String(wc.id);wc.debugger.attach("1.3");
    let keyDispatches=0;
    const send:Send=async(method,params)=>{
      // Scope shortcut isolation to synthetic Agent keys. Keeping it enabled
      // permanently also disables the human's native editing shortcuts.
      const keyEvent=method==="Input.dispatchKeyEvent";
      if(keyEvent && keyDispatches++===0)wc.setIgnoreMenuShortcuts(true);
      try {return await wc.debugger.sendCommand(method,params);}
      finally {if(keyEvent && --keyDispatches===0 && !wc.isDestroyed())wc.setIgnoreMenuShortcuts(false);}
    };
    const tab:WebsiteTab={id,view,send:trackInputs(send),url:()=>wc.getURL()===homeUrl?"about:blank":wc.getURL(),title:()=>wc.getTitle()};
    view.setBounds({x:0,y:0,width:1180,height:752});
    this.views.set(id,tab);
    if(this.window && !this.detachedWhileMinimized)this.window.contentView.addChildView(view,0);
    wc.once("destroyed",()=>{this.views.delete(id);this.changed();});
    // Reapply after navigation so a detached, minimized target can capture its first frame.
    wc.on("did-finish-load",()=>wc.setBackgroundThrottling(false));
    wc.on("content-bounds-updated",event=>event.preventDefault());
    wc.on("did-navigate",()=>this.changed());
    wc.on("did-navigate-in-page",()=>this.changed());
    wc.on("did-start-loading",()=>this.changed());
    wc.on("did-stop-loading",()=>this.changed());
    wc.on("page-title-updated",()=>this.changed());
    wc.on("did-fail-load",(_event,code)=>{if(code!==-3){this.error="This page could not be loaded. Check your connection or try another address.";this.changed();}});
    wc.setWindowOpenHandler(({url:target})=>{try{this.checkedUrl(target);if(this.preview)this.preview.onPopup(target);else void this.open(target).catch(error=>console.error("Website popup failed",error));}catch{/* Unsupported website popups are denied. */}return {action:"deny"};});
    const navigation=(event:Electron.Event,target:string)=>{try{
      if(target!==homeUrl)this.checkedUrl(target);
      if(this.preview && wc.getURL().startsWith("talos-preview:") && /^https?:/.test(target)){event.preventDefault();this.preview.onPopup(target);}
    }catch{event.preventDefault();}};
    wc.on("will-navigate",navigation);wc.on("will-redirect",navigation);
    this.changed();check();await wc.loadURL(url==="about:blank"?homeUrl:this.checkedUrl(url));check();this.changed();return tab;
  }
  attach(window:BaseWindow|undefined):void {
    if(this.window===window)return;
    this.window?.removeListener("minimize",this.synchronizeViews);
    this.window?.removeListener("restore",this.synchronizeViews);
    this.window?.removeListener("closed",this.windowClosed);
    for(const tab of this.views.values())if(tab.view && this.window && !this.window.isDestroyed())this.window.contentView.removeChildView(tab.view);
    this.window=window;
    window?.on("minimize",this.synchronizeViews);window?.on("restore",this.synchronizeViews);window?.once("closed",this.windowClosed);
    this.synchronizeViews();
  }
  present(id:string|null,bounds:DesktopBrowserBounds|null):void {
    this.presentation=id && bounds ? {id,bounds} : undefined;
    // Background targets stay below the opaque product renderer while attached.
    // Native setVisible(false) suspends hit-test/compositor surfaces.
    this.synchronizeViews();
  }
  async navigate(id:string,action:"navigate"|"back"|"forward"|"reload",url?:string):Promise<void> {
    const wc=this.views.get(id)?.view?.webContents;if(!wc)throw new Error("Unknown tab.");
    this.error=null;
    if(action==="navigate")await wc.loadURL(this.checkedUrl(url!));
    else if(action==="back" && wc.navigationHistory.canGoBack())wc.navigationHistory.goBack();
    else if(action==="forward" && wc.navigationHistory.canGoForward())wc.navigationHistory.goForward();
    else if(action==="reload")wc.reload();
    this.changed();
  }
  async screenshot(id:string):Promise<string> {
    const wc=this.views.get(id)?.view?.webContents;if(!wc)throw new Error("Unknown tab.");
    let timer:ReturnType<typeof setTimeout>|undefined;
    try {
      const image=await Promise.race([wc.capturePage(undefined,{stayHidden:true,stayAwake:true}),new Promise<never>((_,reject)=>{timer=setTimeout(()=>reject(new Error("Browser capture timed out.")),3000);})]);
      if(image.isEmpty())throw new Error("Browser page could not be captured.");
      return image.toPNG().toString("base64");
    } finally {clearTimeout(timer);}
  }
  async remove(id:string):Promise<void> {
    const tab=this.views.get(id);if(tab?.view){if(this.window && !this.window.isDestroyed())this.window.contentView.removeChildView(tab.view);tab.view.webContents.close({waitForBeforeUnload:false});this.views.delete(id);}
    this.changed();
  }
  async releaseInputs():Promise<void> {for(const tab of this.views.values())await release(tab.send);}
  close():void {
    this.attach(undefined);this.presentation=undefined;
    this.downloadSession?.removeListener("will-download",this.onDownload);this.downloadSession=undefined;
    for(const [item,directory] of this.downloads) {
      this.downloads.delete(item);item.cancel();
      try {rmSync(directory,{recursive:true,force:true});}catch(error){this.failDownload(error);}
    }
    for(const tab of this.views.values())tab.view?.webContents.close({waitForBeforeUnload:false});
    this.views.clear();this.changed();
  }
}
