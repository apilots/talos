import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { DesktopAppServerController } from "./app-server-controller";
const fixture = vi.hoisted(() => ({
  handlers: new Map<string, (...args:unknown[])=>Promise<unknown>>(),
  workspace: { accepts: vi.fn(), contextVersion: 0, unbind: vi.fn(), bind: vi.fn(), request: vi.fn() },
}));
vi.mock("electron", () => ({ ipcMain: { handle: (name:string, handler:(...args:unknown[])=>Promise<unknown>) => fixture.handlers.set(name,handler) }, BrowserWindow: {}, shell: {} }));
vi.mock("./browser-host", () => ({ desktopBrowserHost: { workspace: fixture.workspace } }));
import {desktopWindows} from "./desktop-window";
import type {BaseWindow,WebContentsView} from "electron";
import { registerDesktopIpcHandlers } from "./desktop-ipc";
import { TALOS_IPC } from "./ipc";
const sender = { mainFrame: {},isDestroyed:()=>false };
const event = { sender, senderFrame: sender.mainFrame };
beforeEach(() => {
  vi.clearAllMocks();desktopWindows.clear();fixture.handlers.clear();fixture.workspace.accepts.mockReturnValue(true);fixture.workspace.contextVersion = 0;
  fixture.workspace.unbind.mockImplementation(() => { fixture.workspace.contextVersion++; });
  fixture.workspace.bind.mockImplementation(user => ({ user }));
  vi.spyOn(console,"error").mockImplementation(() => {});
});
afterEach(() => {desktopWindows.clear();vi.restoreAllMocks();});
function setup(invoke=vi.fn(async()=>({ user: { user_id: "verified-alice" } }))) {
  registerDesktopIpcHandlers({ invoke } as unknown as DesktopAppServerController);
  return { invoke, request: fixture.handlers.get(TALOS_IPC.browserRequest)! };
}
it("resolves browser ownership through the authenticated service instead of a supplied user ID",async()=>{
  const { invoke, request } = setup();
  const credentials={credential:{type:"existing_session_token",token:"fixture-token"}};
  expect(await request(event,{kind:"bind",actorCredentials:credentials,userId:"forged-bob"})).toEqual({status:"success",data:{user:"verified-alice"}});
  expect(invoke).toHaveBeenCalledWith("user/current",{},{actorCredentials:credentials});
  expect(fixture.workspace.bind).toHaveBeenCalledExactlyOnceWith("verified-alice");
});
it("rejects website/subframe senders and malformed credentials before resolving an actor",async()=>{
  const {invoke,request}=setup();
  fixture.workspace.accepts.mockReturnValue(false);
  expect(await request(event,{kind:"bind",actorCredentials:null})).toMatchObject({status:"error"});
  fixture.workspace.accepts.mockReturnValue(true);
  expect(await request({...event,senderFrame:{}},{kind:"bind",actorCredentials:null})).toMatchObject({status:"error"});
  expect(await request(event,{kind:"bind",actorCredentials:{credential:{type:"arbitrary",token:"bad"}}})).toMatchObject({status:"error"});
  expect(invoke).not.toHaveBeenCalled();expect(fixture.workspace.bind).not.toHaveBeenCalled();
});
it("does not bind a late authentication result after navigation or a newer binding attempt",async()=>{
  let complete!:(value:{user:{user_id:string}})=>void;
  const {request}=setup(vi.fn(()=>new Promise(resolve=>{complete=resolve;})));
  const pending=request(event,{kind:"bind",actorCredentials:null});
  fixture.workspace.contextVersion++;complete({user:{user_id:"old-actor"}});
  expect(await pending).toMatchObject({status:"error"});expect(fixture.workspace.bind).not.toHaveBeenCalled();
});

it("routes shared Desktop APIs only for registered product views",async()=>{
 const {invoke}=setup(),handler=fixture.handlers.get(TALOS_IPC.invoke)!;
 expect(await handler(event,"user/current",{},{})).toMatchObject({status:"error"});expect(invoke).not.toHaveBeenCalled();
 desktopWindows.add({window:{} as BaseWindow,view:{webContents:sender} as unknown as WebContentsView});
 expect(await handler(event,"user/current",{},{})).toMatchObject({status:"success"});expect(invoke).toHaveBeenCalledOnce();
 expect(await handler({...event,senderFrame:{}},"user/current",{},{})).toMatchObject({status:"error"});expect(invoke).toHaveBeenCalledOnce();
});
