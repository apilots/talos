import {contextBridge,ipcRenderer} from "electron";
import {BROWSER_CHROME_ACTION,BROWSER_CHROME_STATE,type BrowserChromeAction,type BrowserChromeState} from "./browser-window-contract";
contextBridge.exposeInMainWorld("talosBrowser",{
  act:(action:BrowserChromeAction)=>ipcRenderer.send(BROWSER_CHROME_ACTION,action),
  subscribe:(callback:(state:BrowserChromeState|{focusAddress:true})=>void)=>{
    const listener=(_event:Electron.IpcRendererEvent,state:BrowserChromeState|{focusAddress:true})=>callback(state);
    ipcRenderer.on(BROWSER_CHROME_STATE,listener);
    return ()=>ipcRenderer.removeListener(BROWSER_CHROME_STATE,listener);
  },
});
