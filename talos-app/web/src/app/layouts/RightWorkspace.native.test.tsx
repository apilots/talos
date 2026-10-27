// @vitest-environment jsdom
import { act, fireEvent, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import { show } from "./AppLayout.test-utils";
import { useAppStore } from "@/shared/store/app-store";
const native=(revision:number,ids:string[],revealTabId:string|null=null)=>({sessionId:"layout-session",revision,revealTabId,error:null,tabs:ids.map(id=>({id,title:`Browser ${id}`,url:"https://example.test/",back:false,forward:false,loading:false}))});
it("does not open or focus the workspace when Agent pages arrive in the background",()=>{
 show();const office=screen.getByRole("link",{name:"Files page"});office.focus();
 act(()=>useAppStore.getState().syncDesktopBrowser(native(1,["1"])));
 expect(office).toHaveFocus();expect(screen.queryByRole("dialog",{name:"Right workspace"})).not.toBeInTheDocument();
 expect(screen.getByRole("button",{name:"Show workspace (1 open windows)"})).toBeInTheDocument();
});
it("reveals an explicitly requested page, then preserves chat focus through Agent close and navigation",()=>{
 show();act(()=>useAppStore.getState().openWorkspaceWindow({kind:"markdown",fileId:"review"}));
 act(()=>useAppStore.getState().syncDesktopBrowser(native(1,["1"],"1")));
 const workspace=screen.getByRole("dialog",{name:"Right workspace"});expect(within(workspace).getByRole("tab",{name:"Browser 1"})).toHaveFocus();
 const office=screen.getByRole("link",{name:"Files page"});office.focus();
 act(()=>useAppStore.getState().syncDesktopBrowser(native(2,["1","2"])));expect(office).toHaveFocus();
 act(()=>useAppStore.getState().syncDesktopBrowser(native(3,["2"])));expect(office).toHaveFocus();
 fireEvent.click(within(workspace).getByRole("tab",{name:"Browser 2"}));expect(within(workspace).getByRole("tab",{name:"Browser 2"})).toHaveFocus();
});

it.each(["tab", "close"])("recovers the surviving tab when Agent removal retires its focused %s", kind => {
 show();act(()=>useAppStore.getState().syncDesktopBrowser(native(1,["1","2"],"1")));
 const workspace=screen.getByRole("dialog",{name:"Right workspace"});
 within(workspace).getByRole(kind==="tab"?"tab":"button",{name:kind==="tab"?"Browser 1":"Close Browser 1"}).focus();
 act(()=>useAppStore.getState().syncDesktopBrowser(native(2,["2"])));
 expect(within(workspace).getByRole("tab",{name:"Browser 2"})).toHaveFocus();
});
