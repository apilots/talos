import {expect,it} from "vitest";
import {readFileSync} from "node:fs";
import {runInNewContext} from "node:vm";
import type {BrowserChromeAction,BrowserChromeState} from "./browser-window-contract";

function toolbar() {
  type Listener=(event:{preventDefault():void})=>void;
  const document:{activeElement:Element|null;querySelector(selector:string):Element}={activeElement:null,querySelector:selector=>elements.get(selector)!};
  class Element {
    value="";textContent="";disabled=false;children:Element[]=[];
    readonly listeners=new Map<string,Listener>();
    readonly classList={toggle:()=>{}};
    addEventListener(name:string,listener:Listener){this.listeners.set(name,listener);}
    focus(){document.activeElement=this;}
    select(){}
    blur(){document.activeElement=null;this.listeners.get("blur")?.({preventDefault(){}});}
    fire(name:string){this.listeners.get(name)?.({preventDefault(){}});}
  }
  const elements=new Map(["#address","#address-form","#back","#forward","#reload","#new","#hide","#error","#list"].map(id=>[id,new Element()]));
  let publish:(state:BrowserChromeState)=>void=()=>{};
  const actions:BrowserChromeAction[]=[];
  const address=elements.get("#address")!;
  const state=(url:string):BrowserChromeState=>({tabs:[],address:url,back:false,forward:false,loading:false,error:null});
  const bridge={subscribe:(callback:typeof publish)=>{publish=callback;},act:(action:BrowserChromeAction)=>{
    actions.push(action);
    if(action.kind==="navigate") {
      // Native sibling-view focus loss can fire blur while the input remains
      // document.activeElement in the toolbar's separate WebContents.
      address.fire("blur");publish(state(action.url+"/"));
    }
  }};
  runInNewContext(readFileSync(new URL("../resources/browser-window/chrome.js",import.meta.url),"utf8"),{window:{talosBrowser:bridge},document});
  return {address,form:elements.get("#address-form")!,actions,publish:(url:string)=>publish(state(url))};
}

it("shows the committed address after Enter navigation transfers focus to the website",()=>{
  const ui=toolbar();ui.publish("");ui.address.focus();ui.address.value=" https://talos.love ";
  ui.form.fire("submit");
  expect(ui.actions.at(-1)).toEqual({kind:"navigate",url:"https://talos.love"});
  expect(ui.address.value).toBe("https://talos.love/");
});

it("preserves an address being edited until the user leaves the input",()=>{
  const ui=toolbar();ui.publish("https://talos.love/");ui.address.focus();ui.address.value="https://example.test/draft";
  ui.publish("https://talos.love/#features");
  expect(ui.address.value).toBe("https://example.test/draft");
  ui.address.blur();expect(ui.address.value).toBe("https://talos.love/#features");
});
