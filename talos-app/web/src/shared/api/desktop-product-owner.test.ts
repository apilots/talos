import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { TalosWebClient } from "./client";
import { desktopProductOwner, withManagementTimeout } from "./desktop-product-owner";
import { managedWorkerKey, sessionDetailKey } from "./product-queries";

function fixture(selected?:string,signedIn=true){
 const local={sessions:{get:vi.fn().mockResolvedValue({session:{id:"session",nodeId:"local"}})}};
 const scoped={sessions:{get:vi.fn().mockResolvedValue({session:{id:"session",nodeId:"remote"}})}};
 const cloud={forComputer:vi.fn(()=>scoped)};
 const queries=new QueryClient({defaultOptions:{queries:{retry:false}}});
 const owner=desktopProductOwner(local as unknown as TalosWebClient,cloud as unknown as TalosWebClient,queries,"account",true,signedIn,undefined,()=>Promise.resolve("local"),selected);
 return {owner,local,cloud,scoped,queries};
}
describe("Computer product ownership",()=>{
 it("keeps the current registered Computer local without Sync",async()=>{
  const f=fixture("local");expect(await f.owner.resolve({nodeId:"local"})).toBe(f.local);
  expect(f.cloud.forComputer).not.toHaveBeenCalled();
 });
 it("routes every other Computer through its scoped Server API",async()=>{
  const f=fixture("remote");expect(await f.owner.resolve({nodeId:"remote"})).toBe(f.scoped);
  expect(f.cloud.forComputer).toHaveBeenCalledWith("remote");
 });
 it("reads a selected remote Session without probing the local replica",async()=>{
  const f=fixture("remote");expect((await f.owner.readSession("session")).owner).toBe("server");
  expect(f.local.sessions.get).not.toHaveBeenCalled();
 });
 it("does not fall back to a local replica when the remote API fails",async()=>{
  const f=fixture("remote");f.scoped.sessions.get.mockRejectedValue(new Error("offline"));
  await expect(f.owner.readSession("session")).rejects.toThrow("offline");
  expect(f.local.sessions.get).not.toHaveBeenCalled();
 });
 it("keeps unselected local history available offline",async()=>{
  const f=fixture(undefined,false);expect((await f.owner.readSession("session")).owner).toBe("desktop");
 });
 it("requires authentication for a remote product",async()=>{
  const f=fixture("remote",false);await expect(f.owner.resolve({nodeId:"remote"})).rejects.toMatchObject({code:"unauthenticated"});
 });
 it("uses the existing Session binding after another Computer is selected",async()=>{
  const f=fixture("other");
  f.queries.setQueryData(sessionDetailKey("account","session"),{session:{id:"session",nodeId:"remote"}});
  await f.owner.resolve({sessionId:"session"});
  expect(f.cloud.forComputer).toHaveBeenLastCalledWith("remote");
 });
 it("keeps a known Talo Session on its Server while a personal Computer is selected",async()=>{
  const f=fixture("remote");
  f.queries.setQueryData(managedWorkerKey("account"),{instance:{nodeId:"talo",conversationSessionId:"managed"}});
  await f.owner.resolve({sessionId:"managed"});
  expect(f.cloud.forComputer).toHaveBeenLastCalledWith("talo");
 });
 it("preserves explicit targets over cached Session metadata",async()=>{
  const f=fixture("other");
  f.queries.setQueryData(sessionDetailKey("account","session"),{session:{id:"session",nodeId:"remote"}});
  await f.owner.resolve({sessionId:"session",nodeId:"explicit"});
  expect(f.cloud.forComputer).toHaveBeenLastCalledWith("explicit");
 });
 it("bounds management requests without swallowing their errors",async()=>{
  await expect(withManagementTimeout(()=>Promise.reject(new Error("failed")))).rejects.toThrow("failed");
 });
});
