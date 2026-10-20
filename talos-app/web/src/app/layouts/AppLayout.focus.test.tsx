// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useAppStore } from "@/shared/store/app-store";
import { fixture, show, open } from "./AppLayout.test-utils";

it.each([true, false])("does not focus navigation on initial wide render with sidebarOpen=%s", sidebarOpen => {
  useAppStore.setState({ sidebarOpen });show("/");
  expect(document.body).toHaveFocus();
  expect(screen.getByRole("button", { name: sidebarOpen ? "Collapse sidebar" : "Open sidebar" })).not.toHaveFocus();
});
it.each([true, false])("does not acquire sidebar focus during StrictMode's initial replay with sidebarOpen=%s", sidebarOpen => {
  useAppStore.setState({sidebarOpen});show("/",true);
  expect(document.body).toHaveFocus();
});
it("does not close or restore focus from a connected narrow modal during StrictMode replay", () => {
  fixture.wide=false;useAppStore.setState({sidebarOpen:true});
  const showModal=vi.spyOn(HTMLDialogElement.prototype,"showModal");
  const close=vi.spyOn(HTMLDialogElement.prototype,"close");
  show("/",true);
  fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
  expect(showModal).toHaveBeenCalledTimes(1);expect(close).not.toHaveBeenCalled();
  expect(screen.getByRole("dialog",{name:"Projects and chats"})).toHaveAttribute("open");
});
it("preserves an existing main-content focus while opening and closing the wide sidebar", () => {
  show();const target=screen.getByRole("link",{name:"Files page"});target.focus();
  act(()=>useAppStore.getState().setSidebarOpen(true));expect(target).toHaveFocus();
  act(()=>useAppStore.getState().setSidebarOpen(false));expect(target).toHaveFocus();
});
it.each([true,false])("starts narrow without acquiring modal focus despite sidebarOpen=%s", sidebarOpen => {
  fixture.wide=false;useAppStore.setState({sidebarOpen});show("/");
  expect(document.body).toHaveFocus();
  expect(screen.getByRole("button",{name:"Open sidebar"})).not.toHaveFocus();
  expect(screen.queryByRole("dialog",{name:"Projects and chats"})).not.toBeInTheDocument();
});
it("restores a focused narrow drawer to its live navigation toggle when widening", () => {
  fixture.wide=false;useAppStore.setState({sidebarOpen:true});show();
  fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));
  const panel=screen.getByRole("dialog",{name:"Projects and chats"});
  within(panel).getByRole("button",{name:"Sidebar action"}).focus();
  fixture.wide=true;fireEvent(window,new Event("resize"));
  expect(screen.queryByRole("dialog",{name:"Projects and chats"})).not.toBeInTheDocument();
  expect(screen.getByRole("button",{name:"Collapse sidebar"})).toHaveFocus();
});
it.each(["address","iframe"])("retains live %s focus while the same workspace changes presentation", kind => {
  show();act(()=>useAppStore.getState().createWorkspaceWebpage());
  const address=screen.getByRole("textbox",{name:"Preview address"});
  const frame=screen.getByTitle("Preview content") as HTMLIFrameElement;
  frame.contentDocument!.body.innerHTML='<input aria-label="Frame draft">';
  const field=frame.contentDocument!.querySelector("input")!;
  const focusContext=vi.spyOn(frame.contentWindow!,"focus").mockImplementation(()=>frame.focus());
  if(kind==="address") address.focus();else field.focus();
  for(const wide of [false,true]){
    fixture.wide=wide;fireEvent(window,new Event("resize"));
    expect(document.activeElement).toBe(kind==="address" ? address : frame);
    if(kind==="iframe") {
      if(wide) expect(frame.contentDocument!.activeElement).toBe(field);
      else field.focus();
    }
  }
  if(kind==="iframe") expect(focusContext).toHaveBeenCalledTimes(2);
});

it("immediately retires sidebar focus on close without waiting for its visual transition",()=>{
 useAppStore.setState({sidebarOpen:true});show();
 const action=screen.getByRole("button",{name:"Sidebar action"});action.focus();
 act(()=>useAppStore.getState().setSidebarOpen(false));
 expect(action.closest('[aria-label="Projects and chats"]')).toHaveAttribute("inert");
 expect(screen.getByRole("button",{name:"Open sidebar"})).toHaveFocus();
});

it("does not reopen a covered narrow sidebar after the workspace closes",()=>{
 fixture.wide=false;useAppStore.setState({sidebarOpen:true});show();
 fireEvent.click(screen.getByRole("button",{name:"Open sidebar"}));open("worker");
 expect(useAppStore.getState().sidebarOpen).toBe(true);
 const panel=screen.getByRole("dialog",{name:"Right workspace"});
 within(panel).getByRole("button",{name:"Worker action"}).focus();
 fireEvent(panel,new Event("cancel",{cancelable:true}));
 expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
 expect(screen.getByRole("button",{name:/^Show workspace/})).toHaveFocus();
 fixture.wide=true;fireEvent(window,new Event("resize"));
 expect(screen.getByRole("button",{name:"Collapse sidebar"})).toBeInTheDocument();
 expect(screen.getByRole("button",{name:/^Show workspace/})).toHaveFocus();
});
