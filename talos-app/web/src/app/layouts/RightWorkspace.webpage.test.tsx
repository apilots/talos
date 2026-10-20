// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { useRef } from "react";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import "@/shared/i18n/setup";
import { useAppStore } from "@/shared/store/app-store";
import { WorkspaceControls } from "./AppTopBar";
import { RightWorkspace } from "./RightWorkspace";
vi.mock("@/features/computers", () => ({
  useProductScope: () => "webpage-test",
  useComputer: () => ({ data: { displayName: "Worker" } }),
  ComputerDesktop: () => <section aria-label="Worker media" />,
}));
vi.mock("@/features/files", () => ({ useFileMetadata: () => ({ data: undefined }), MarkdownFilePreview: () => null }));
function Harness() {
  const toggle=useRef<HTMLButtonElement>(null);
  const windows=useAppStore(state=>state.workspaceWindows);
  const open=useAppStore(state=>state.workspaceOpen);
  const expanded=useAppStore(state=>state.workspaceExpanded);
  return <>{!open?<WorkspaceControls toggleRef={toggle}/>:null}{windows.length?<RightWorkspace width={600} maxWidth={900} modal={expanded} toggleRef={toggle}/>:null}</>;
}
beforeEach(()=>{
 HTMLDialogElement.prototype.showModal=function(){this.open=true;};HTMLDialogElement.prototype.close=function(){this.open=false;};
 useAppStore.getState().setConnection({userId:"webpage-owner"});
 useAppStore.setState({workspaceWindows:[],workspaceActiveKey:null,workspaceOpen:false,workspaceExpanded:false});
});
afterEach(cleanup);
it("preserves the actual webpage iframe and address draft across resource switching and collapse",()=>{
 render(<Harness/>);fireEvent.click(screen.getByRole("button",{name:"New webpage tab"}));
 const first=useAppStore.getState().workspaceActiveKey!;
 const address=screen.getByRole("textbox",{name:"Web address"});
 fireEvent.change(address,{target:{value:"http://localhost:3010/"}});fireEvent.submit(address.closest("form")!);
 const frame=screen.getByTitle("Web preview: http://localhost:3010/");
 const panel=frame.closest('[role="tabpanel"]')!;
 fireEvent.change(address,{target:{value:"unsubmitted address draft"}});
 act(()=>useAppStore.getState().openWorkspaceWindow({kind:"computer",nodeId:"worker"}));
 expect(frame.isConnected).toBe(true);expect(panel).toHaveAttribute("hidden");expect(panel).toHaveAttribute("inert");
 act(()=>useAppStore.getState().selectWorkspaceWindow(first));
 expect(screen.getByTitle("Web preview: http://localhost:3010/")).toBe(frame);
 expect(screen.getByRole("textbox",{name:"Web address"})).toHaveValue("unsubmitted address draft");
 fireEvent.click(screen.getByRole("button",{name:"Expand workspace"}));
 expect(screen.getByTitle("Web preview: http://localhost:3010/")).toBe(frame);
 fireEvent.click(screen.getByRole("button",{name:"Restore workspace"}));
 expect(screen.getByTitle("Web preview: http://localhost:3010/")).toBe(frame);
 fireEvent.click(screen.getByRole("button",{name:/^Hide workspace/}));
 expect(frame.isConnected).toBe(true);expect(panel).toHaveAttribute("hidden");expect(panel).toHaveAttribute("inert");
 expect(screen.getByRole("button",{name:/^Show workspace/})).toHaveFocus();
 fireEvent.click(screen.getByRole("button",{name:/^Show workspace/}));
 expect(screen.getByTitle("Web preview: http://localhost:3010/")).toBe(frame);
 expect(screen.getByRole("textbox",{name:"Web address"})).toHaveValue("unsubmitted address draft");
});
it("keeps same-URL tabs independent and truly removes their iframes on close or account change",()=>{
 render(<Harness/>);const store=useAppStore.getState();
 act(()=>store.createWorkspaceWebpage());const first=useAppStore.getState().workspaceActiveKey!;
 act(()=>store.navigateWorkspaceWebpage(first,"https://example.test/"));
 const frame=screen.getByTitle("Web preview: https://example.test/");
 act(()=>store.createWorkspaceWebpage());const second=useAppStore.getState().workspaceActiveKey!;
 act(()=>store.navigateWorkspaceWebpage(second,"https://example.test/"));
 expect(screen.getAllByTitle("Web preview: https://example.test/")).toHaveLength(2);
 act(()=>store.closeWorkspaceWindow(first));expect(frame.isConnected).toBe(false);
 const secondFrame=screen.getByTitle("Web preview: https://example.test/");
 act(()=>store.setConnection({userId:"different-owner"}));
 expect(secondFrame.isConnected).toBe(false);expect(useAppStore.getState().workspaceWindows).toHaveLength(0);
});
it("returns focus to the workspace toggle after native dialog close restores a previous opener",()=>{
 render(<><button type="button">Previous opener</button><Harness/></>);
 const opener=screen.getByRole("button",{name:"Previous opener"});
 HTMLDialogElement.prototype.close=function(){this.open=false;opener.focus();};
 fireEvent.click(screen.getByRole("button",{name:"New webpage tab"}));
 fireEvent.click(screen.getByRole("button",{name:"Expand workspace"}));
 const panel=screen.getByRole("dialog",{name:"Right workspace"});
 screen.getByRole("tab").focus();
 fireEvent(panel,new Event("cancel",{cancelable:true}));
 expect(screen.getByRole("button",{name:/^Show workspace/})).toHaveFocus();
});
