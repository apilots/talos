/** Private browser-chrome UI only; never exposed to website views or app-server tools. */
export const BROWSER_CHROME_ACTION = "talos:browser-chrome-action";
export const BROWSER_CHROME_STATE = "talos:browser-chrome-state";
export type BrowserChromeAction =
  | {kind: "state" | "back" | "forward" | "reload" | "new" | "hide"}
  | {kind: "navigate"; url: string}
  | {kind: "focus" | "close"; tabId: string};
export type BrowserChromeState = {
  tabs: Array<{id:string;title:string;active:boolean}>;
  address:string; back:boolean; forward:boolean; loading:boolean; error:string|null;
};
export function parseBrowserChromeAction(value:unknown):BrowserChromeAction {
  if(!value || typeof value!=="object") throw new Error("Invalid browser navigation action.");
  const action=value as Record<string,unknown>;
  switch(action.kind) {
    case "state":case "back":case "forward":case "reload":case "new":case "hide":return {kind:action.kind};
    case "navigate":if(typeof action.url==="string" && action.url.length<=8192)return {kind:"navigate",url:action.url};break;
    case "focus":case "close":if(typeof action.tabId==="string" && /^\d+$/.test(action.tabId))return {kind:action.kind,tabId:action.tabId};break;
  }
  throw new Error("Invalid browser navigation action.");
}
