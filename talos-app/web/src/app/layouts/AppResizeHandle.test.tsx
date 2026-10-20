// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AppResizeHandle } from "./AppResizeHandle";
beforeEach(()=>{
 HTMLElement.prototype.setPointerCapture=vi.fn();HTMLElement.prototype.hasPointerCapture=vi.fn(()=>true);HTMLElement.prototype.releasePointerCapture=vi.fn();
});
afterEach(cleanup);
function show(direction:1|-1=1){
 const resize=vi.fn(),dragging=vi.fn();
 const result=render(<AppResizeHandle label="Resize pane" controls="pane" value={300} min={224} max={480} direction={direction} onResize={resize} onDragging={dragging} className="inset-y-0"/>);
 return { ...result,resize,dragging,handle:screen.getByRole("separator",{name:"Resize pane"}) };
}
it("captures the pointer, clamps live updates and releases capture on cancel",()=>{
 const {handle,resize,dragging}=show();
 fireEvent.pointerDown(handle,{pointerId:7,button:0,clientX:100});
 expect(handle.setPointerCapture).toHaveBeenCalledWith(7);expect(dragging).toHaveBeenLastCalledWith(true);
 fireEvent.pointerMove(handle,{pointerId:8,clientX:300});expect(resize).not.toHaveBeenCalled();
 fireEvent.pointerMove(handle,{pointerId:7,clientX:500});expect(resize).toHaveBeenLastCalledWith(480);
 fireEvent.pointerCancel(handle,{pointerId:7});expect(handle.releasePointerCapture).toHaveBeenCalledWith(7);expect(dragging).toHaveBeenLastCalledWith(false);
 resize.mockClear();fireEvent.pointerMove(handle,{pointerId:7,clientX:50});expect(resize).not.toHaveBeenCalled();
});
it("clears active drag on capture loss and unmount",()=>{
 const {handle,dragging,unmount}=show();
 fireEvent.pointerDown(handle,{pointerId:1,button:0,clientX:100});fireEvent.lostPointerCapture(handle,{pointerId:1});
 expect(dragging).toHaveBeenLastCalledWith(false);
 fireEvent.pointerDown(handle,{pointerId:2,button:0,clientX:100});unmount();expect(dragging).toHaveBeenLastCalledWith(false);
 expect(handle.releasePointerCapture).toHaveBeenCalledWith(2);
});
it("exposes keyboard separator values and accounts for the right pane's reversed edge",()=>{
 const {handle,resize}=show(-1);
 expect(handle).toHaveAttribute("aria-orientation","vertical");expect(handle).toHaveAttribute("aria-valuenow","300");
 fireEvent.keyDown(handle,{key:"ArrowRight"});expect(resize).toHaveBeenLastCalledWith(284);
 fireEvent.keyDown(handle,{key:"ArrowLeft",shiftKey:true});expect(resize).toHaveBeenLastCalledWith(364);
 fireEvent.keyDown(handle,{key:"Home"});expect(resize).toHaveBeenLastCalledWith(224);
 fireEvent.keyDown(handle,{key:"End"});expect(resize).toHaveBeenLastCalledWith(480);
});
