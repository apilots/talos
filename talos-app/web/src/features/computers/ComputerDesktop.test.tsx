// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { BrowserSnapshot, DesktopViewGrant } from "@talos/sdk-core";
import { i18n } from "@/shared/i18n/setup";
import { ComputerDesktopButton } from "./ComputerDesktop";
import { useAppStore } from "@/shared/store/app-store";
import { RightWorkspace } from "@/app/layouts/RightWorkspace";
import { AppNavigation, AppTopBar, WorkspaceControls } from "@/app/layouts/AppTopBar";
import { useRef } from "react";
import { MemoryRouter } from "react-router-dom";

const fixture=vi.hoisted(()=>({scope:"alice",operate:vi.fn(),open:vi.fn(),socket:vi.fn(),rfbs:[] as Array<EventTarget & {disconnect:ReturnType<typeof vi.fn>;viewOnly:boolean;focus:ReturnType<typeof vi.fn>}>}));
vi.mock("./hooks",()=>({useProductScope:()=>fixture.scope,useComputers:()=>({localId:"worker"}),useComputer:(nodeId:string)=>({data:{displayName:nodeId,available:true}}),useSelectedComputerId:()=>"worker"}));
vi.mock("@/features/chat",()=>({useSessionQuery:()=>({data:undefined}),useProjectsQuery:()=>({data:[]})}));
function WorkspaceHarness() {
 const windows=useAppStore(state=>state.workspaceWindows),expanded=useAppStore(state=>state.workspaceExpanded),visible=useAppStore(state=>state.workspaceOpen);
 const toggle=useRef<HTMLButtonElement>(null),sidebar=useRef<HTMLButtonElement>(null);
 const navigation=<AppNavigation sidebarVisible={false} sidebarAvailable={false} sidebarToggleRef={sidebar}/>;
 return <MemoryRouter><ComputerDesktopButton nodeId="worker" online/><ComputerDesktopButton nodeId="worker" online remoteOnly label="Your computer"/>{!expanded?<AppTopBar>{navigation}{!visible?<WorkspaceControls toggleRef={toggle}/>:null}</AppTopBar>:null}{windows.length?<RightWorkspace modal={expanded} toggleRef={toggle} width={600} maxWidth={900}/>:null}</MemoryRouter>;
}
vi.mock("@/shared/api/desktop-client",()=>({isDesktopRuntime:()=>true,getDesktopBridge:()=>null}));
vi.mock("@/shared/api/client",()=>({
  getTalosClient:()=>({browser:{operate:fixture.operate},desktop:{open:fixture.open,connect:()=>fixture.socket()}}),
}));
vi.mock("@novnc/novnc/lib/rfb",()=>({default:class extends EventTarget {
  viewOnly=true; scaleViewport=false;resizeSession=false;qualityLevel=0;compressionLevel=0;
  disconnect=vi.fn(()=>this.dispatchEvent(new Event("disconnect")));
  focus:ReturnType<typeof vi.fn>;
  constructor(element:HTMLElement){super();const canvas=document.createElement("canvas");canvas.tabIndex=-1;element.append(canvas);this.focus=vi.fn(()=>canvas.focus());fixture.rfbs.push(this);queueMicrotask(()=>this.dispatchEvent(new Event("connect")));}
}}));
beforeEach(()=>{
 vi.stubGlobal("matchMedia",()=>({matches:true,addEventListener:vi.fn(),removeEventListener:vi.fn()}));
 HTMLDialogElement.prototype.showModal=function(){this.open=true;};
 HTMLDialogElement.prototype.close=function(){this.open=false;};
});
function snapshot(host:"docker"|"desktop"="docker"):BrowserSnapshot {
  return {enabled:true,host,selectedTargetId:"managed",targets:[],display:host==="docker"?{kind:"dockerDesktop",targetId:"managed"}:{kind:"unavailable",reason:"nativeHost"},
    control:{generation:"generation",revision:1,epoch:0,owner:{kind:"idle"}}};
}
function grant(inputMode:"readOnly"|"control",index=0):DesktopViewGrant {
  return {ticket:`ticket-${index}`,streamId:`stream-${index}`,viewerId:"viewer",generation:"generation",inputMode,
    controllerId:inputMode==="control"?"controller":null,inputEpoch:inputMode==="control"?1:null,expiresAt:9999999999};
}
function show(value:BrowserSnapshot) {
  fixture.operate.mockResolvedValue(value);
  fixture.open.mockResolvedValue(grant("readOnly"));
  fixture.socket.mockImplementation(()=>{
    const socket={binaryType:"",onopen:null as null|(()=>void),onerror:null,close:vi.fn()};
    queueMicrotask(()=>socket.onopen?.());return socket;
  });
  const query=new QueryClient({defaultOptions:{queries:{retry:false}}});
  const view=render(<QueryClientProvider client={query}><WorkspaceHarness/></QueryClientProvider>);
  return {query,...view};
}
afterEach(async()=>{cleanup();useAppStore.setState({workspaceWindows:[],workspaceActiveKey:null,workspaceOpen:false,workspaceExpanded:false});fixture.rfbs.length=0;fixture.scope="alice";vi.unstubAllGlobals();vi.resetAllMocks();await i18n.changeLanguage("en");});

it("does not advertise or request a remote stream for a native Desktop",async()=>{
  const {query}=show(snapshot("desktop"));
  await waitFor(()=>expect(fixture.operate).toHaveBeenCalled());
  expect(screen.queryByRole("button",{name:"View computer"})).not.toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"Your computer"})).not.toBeInTheDocument();
  act(()=>useAppStore.getState().openWorkspaceWindow({kind:"computer",nodeId:"worker"}));
  await screen.findByText("Browser unavailable");
  expect(fixture.open).not.toHaveBeenCalled();query.clear();
});
it("keeps the same read-only stream through a locale change and closes it on an account switch",async()=>{
  const {query,rerender}=show(snapshot());
  fireEvent.click(await screen.findByRole("button",{name:"View computer"}));
  await screen.findByText("Agent can operate");
  expect(fixture.rfbs).toHaveLength(1);
  await act(()=>i18n.changeLanguage("zh-CN"));
  await screen.findByText("Agent 可操作");
  expect(fixture.open).toHaveBeenCalledTimes(1);
  expect(fixture.rfbs).toHaveLength(1);
  fixture.scope="bob";
  act(()=>useAppStore.getState().setConnection({userId:"bob"}));
  rerender(<QueryClientProvider client={query}><WorkspaceHarness/></QueryClientProvider>);
  await waitFor(()=>expect(useAppStore.getState().workspaceWindows).toHaveLength(0));
  expect(fixture.rfbs[0].disconnect).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("dialog",{name:"Worker 电脑"})).not.toBeInTheDocument();query.clear();
});
it("requests a fresh immutable stream for takeover instead of upgrading the old RFB connection",async()=>{
  const value=snapshot();const {query}=show(value);
  expect(await screen.findByRole("button",{name:"Your computer"})).toBeInTheDocument();
  fireEvent.click(await screen.findByRole("button",{name:"View computer"}));
  await screen.findByText("Agent can operate");
  fixture.operate.mockImplementation(async({operation})=>{
    if(operation.kind==="takeControl") {
      value.control={...value.control,revision:2,epoch:1,owner:{kind:"handingOver",controllerId:"controller"}};
      return structuredClone(value);
    }
    if(fixture.rfbs.length===2)value.control.owner={kind:"human",controllerId:"controller"};
    return structuredClone(value);
  });
  fixture.open.mockImplementation(async(request)=>grant(request.inputMode,1));
  fireEvent.click(screen.getByRole("button",{name:"Take control"}));
  await screen.findByRole("button",{name:"Return to agent"});
  expect(fixture.open).toHaveBeenLastCalledWith(expect.objectContaining({nodeId:"worker",inputMode:"control",controllerId:"controller",inputEpoch:1}));
  expect(fixture.rfbs).toHaveLength(2);
  expect(fixture.rfbs[0].disconnect).toHaveBeenCalledTimes(1);
  expect(fixture.rfbs[0].viewOnly).toBe(true);
  await waitFor(()=>expect(fixture.rfbs[1].viewOnly).toBe(false));expect(fixture.rfbs[1].focus).toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button",{name:"Expand workspace"}));
  fireEvent.click(screen.getByRole("button",{name:"Restore workspace"}));
  expect(fixture.rfbs).toHaveLength(2);expect(fixture.rfbs[1].disconnect).not.toHaveBeenCalled();
  expect(fixture.rfbs[1].viewOnly).toBe(false);
  fireEvent.click(screen.getByRole("button",{name:"Hide workspace (1 open windows)"}));
  expect(fixture.rfbs[1].disconnect).toHaveBeenCalledTimes(1);
  expect(useAppStore.getState().workspaceWindows).toHaveLength(1);
  expect(fixture.operate.mock.calls.some(([request])=>request.operation.kind==="resume"||request.operation.kind==="releaseControl")).toBe(false);
  fireEvent.click(screen.getByRole("button",{name:"Show workspace (1 open windows)"}));
  await waitFor(()=>expect(fixture.rfbs).toHaveLength(3));
  expect(fixture.open).toHaveBeenLastCalledWith(expect.objectContaining({inputMode:"readOnly",controllerId:null}));
  expect(fixture.rfbs[2].viewOnly).toBe(true);query.clear();
});

function deferred<T>() { let resolve!:(value:T)=>void;const promise=new Promise<T>(done=>{resolve=done;});return{promise,resolve}; }
it("ignores an old generation grant that finishes after the current grant",async()=>{
 const first=snapshot(),second=snapshot();second.control={...second.control,generation:"new-generation",revision:2};
 const {query}=show(first);const old=deferred<DesktopViewGrant>(),current=deferred<DesktopViewGrant>();
 fixture.open.mockReturnValueOnce(old.promise).mockReturnValueOnce(current.promise);
 fireEvent.click(await screen.findByRole("button",{name:"View computer"}));await waitFor(()=>expect(fixture.open).toHaveBeenCalledTimes(1));
 fixture.operate.mockResolvedValue(second);act(()=>query.setQueryData(["talos","alice","browser","worker"],second));
 await waitFor(()=>expect(fixture.open).toHaveBeenCalledTimes(2));
 await act(async()=>current.resolve({...grant("readOnly",2),generation:"new-generation"}));await screen.findByText("Agent can operate");
 await act(async()=>old.resolve(grant("readOnly",1)));
 expect(fixture.rfbs).toHaveLength(1);expect(fixture.rfbs[0].disconnect).not.toHaveBeenCalled();query.clear();
});
it("retires a pending grant when Browser is disabled",async()=>{
 const value=snapshot();const {query}=show(value);const pending=deferred<DesktopViewGrant>();fixture.open.mockReturnValueOnce(pending.promise);
 fireEvent.click(await screen.findByRole("button",{name:"View computer"}));await waitFor(()=>expect(fixture.open).toHaveBeenCalledTimes(1));
 const disabled={...value,enabled:false};fixture.operate.mockResolvedValue(disabled);act(()=>query.setQueryData(["talos","alice","browser","worker"],disabled));
 await screen.findByText("Desktop unavailable");await act(async()=>pending.resolve(grant("readOnly")));
 expect(fixture.socket).not.toHaveBeenCalled();query.clear();
});
it("keeps the same stream through expand/collapse and restores keyboard focus on close",async()=>{
 const {query}=show(snapshot());const trigger=await screen.findByRole("button",{name:"View computer"});trigger.focus();fireEvent.click(trigger);
 await screen.findByText("Agent can operate");expect(screen.getByRole("tab",{name:"worker"})).toHaveFocus();
 fireEvent.click(screen.getByRole("button",{name:"Expand workspace"}));
 expect(screen.getByRole("button",{name:"Restore workspace"})).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"Restore workspace"}));
 expect(fixture.rfbs).toHaveLength(1);expect(fixture.open).toHaveBeenCalledTimes(1);
 const remote=screen.getByRole("group",{name:"Remote Worker desktop"});expect(remote).toHaveAttribute("tabindex","0");
 remote.focus();fireEvent.keyDown(remote,{key:"Escape",ctrlKey:true,altKey:true});
 expect(screen.getByRole("button",{name:"Hide workspace (1 open windows)"})).toHaveFocus();fireEvent.click(screen.getByRole("button",{name:"Hide workspace (1 open windows)"}));
 expect(screen.getByRole("button",{name:"Show workspace (1 open windows)"})).toHaveFocus();query.clear();
});

it("keeps pause/resume in the session viewer and preserves read-only pixels while pausing",async()=>{
 const value=snapshot();const {query}=show(value);
 fireEvent.click(await screen.findByRole("button",{name:"View computer"}));await screen.findByText("Agent can operate");
 fixture.operate.mockImplementation(async({operation})=>{
  if(operation.kind==="pause")value.control={...value.control,revision:2,epoch:1,owner:{kind:"suspended",reason:"userPaused"}};
  if(operation.kind==="resume")value.control={...value.control,revision:3,epoch:2,owner:{kind:"idle"}};
  return structuredClone(value);
 });
 fireEvent.click(screen.getByRole("button",{name:"Pause agent"}));await screen.findByRole("button",{name:"Resume agent"});
 expect(fixture.rfbs).toHaveLength(1);expect(fixture.rfbs[0].disconnect).not.toHaveBeenCalled();
 fireEvent.click(screen.getByRole("button",{name:"Resume agent"}));await screen.findByRole("button",{name:"Pause agent"});
 expect(fixture.operate).toHaveBeenCalledWith({operation:{kind:"resume",expectedRevision:2}});
 expect(fixture.open).toHaveBeenCalledTimes(2);query.clear();
});

it("keeps Desktop browser choice and local actions in the computer page without a remote stream",async()=>{
 const value=snapshot("desktop");value.targets=[{targetId:"managed",kind:"managed",displayName:"Talos browser",readiness:{status:"ready"},localOpen:"desktopOwned"},{targetId:"connected",kind:"connected",displayName:"My Chrome",readiness:{status:"needsConnection"},localOpen:"externalUserWindow"}];
 const {query}=show(value);fireEvent.click(await screen.findByRole("button",{name:"Computer browser"}));
 await screen.findByRole("button",{name:"Open browser"});expect(screen.getAllByRole("radio")).toHaveLength(2);
 fireEvent.click(screen.getByRole("button",{name:"Open browser"}));await waitFor(()=>expect(fixture.operate).toHaveBeenCalledWith({operation:{kind:"openLocal"}}));
 expect(fixture.open).not.toHaveBeenCalled();expect(fixture.socket).not.toHaveBeenCalled();query.clear();
});


it("retires a pending stream on tab switch and reauthorizes the original Computer on return",async()=>{
 const {query}=show(snapshot());const old=deferred<DesktopViewGrant>();fixture.open.mockReturnValueOnce(old.promise);
 fireEvent.click(await screen.findByRole("button",{name:"View computer"}));
 await waitFor(()=>expect(fixture.open).toHaveBeenCalledTimes(1));
 act(()=>useAppStore.getState().openWorkspaceWindow({kind:"computer",nodeId:"second-worker"}));
 await screen.findByText("Agent can operate");
 expect(fixture.rfbs).toHaveLength(1);expect(fixture.open).toHaveBeenCalledTimes(2);
 await act(async()=>old.resolve(grant("readOnly",8)));
 expect(fixture.rfbs).toHaveLength(1);
 fireEvent.click(screen.getByRole("tab",{name:"worker"}));
 await waitFor(()=>expect(fixture.rfbs).toHaveLength(2));
 expect(fixture.rfbs[0].disconnect).toHaveBeenCalledTimes(1);
 expect(fixture.open).toHaveBeenLastCalledWith(expect.objectContaining({nodeId:"worker",inputMode:"readOnly"}));
 expect(fixture.open.mock.calls[0][0].viewerId).not.toBe(fixture.open.mock.calls[2][0].viewerId);
 expect(fixture.operate.mock.calls.some(([request])=>request.operation.kind==="resume")).toBe(false);query.clear();
});
