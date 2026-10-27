import {EventEmitter} from "node:events";
import {afterEach,expect,it,vi} from "vitest";
vi.mock("electron",()=>({
 BaseWindow:class extends EventEmitter {
  destroyed=false;contentView=Object.assign(new EventEmitter(),{addChildView:vi.fn()});getContentSize(){return [1100,700];}isDestroyed(){return this.destroyed;}
  destroy(){this.destroyed=true;this.emit("closed");}
 },
 WebContentsView:class {
  webContents=Object.assign(new EventEmitter(),{isDestroyed:()=>false,close:vi.fn()});setBounds=vi.fn();
 },
}));
import {createDesktopWindow,desktopWindows,isDesktopRenderer} from "./desktop-window";
afterEach(()=>{desktopWindows.clear();});
it("owns a real trusted view with native geometry and excludes other contents",()=>{
 const product=createDesktopWindow({width:1100,height:700},{sandbox:true});
 expect(product.window.contentView.addChildView).toHaveBeenCalledWith(product.view);
 expect(product.view.setBounds).toHaveBeenLastCalledWith({x:0,y:0,width:1100,height:700});
 expect(isDesktopRenderer(product.view.webContents)).toBe(true);
 expect(isDesktopRenderer({isDestroyed:()=>false} as Electron.WebContents)).toBe(false);
 vi.spyOn(product.window,"getContentSize").mockReturnValue([1100,673]);
 product.window.contentView.emit("bounds-changed");expect(product.view.setBounds).toHaveBeenLastCalledWith({x:0,y:0,width:1100,height:673});
});
it("allows the product document to cancel closing and destroys the native window after unload",()=>{
 const {window,view}=createDesktopWindow({},{}),event={preventDefault:vi.fn()};
 window.emit("close",event);expect(event.preventDefault).toHaveBeenCalledOnce();
 expect(view.webContents.close).toHaveBeenCalledWith({waitForBeforeUnload:true});expect(window.isDestroyed()).toBe(false);
 view.webContents.emit("destroyed");expect(window.isDestroyed()).toBe(true);expect(desktopWindows.size).toBe(0);
});
it("closes its trusted contents when the native window is destroyed directly",()=>{
 const {window,view}=createDesktopWindow({},{});window.destroy();
 expect(view.webContents.close).toHaveBeenCalledWith({waitForBeforeUnload:false});expect(isDesktopRenderer(view.webContents)).toBe(false);
});
