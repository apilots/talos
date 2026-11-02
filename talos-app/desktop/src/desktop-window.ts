import { BaseWindow, WebContentsView, type WebContents, type BaseWindowConstructorOptions, type WebPreferences } from "electron";

/** Trusted product surfaces, separate from managed website targets and input modals. */
export const desktopWindows = new Set<{window:BaseWindow;view:WebContentsView}>();
export function isDesktopRenderer(contents:WebContents):boolean {
  return [...desktopWindows].some(product=>product.view.webContents===contents) && !contents.isDestroyed();
}

/** Own the actual product view so websites can be ordered both above and below it. */
export function createDesktopWindow(options:BaseWindowConstructorOptions,webPreferences:WebPreferences) {
  const window=new BaseWindow(options),view=new WebContentsView({webPreferences});
  const contents=view.webContents;
  const product={window,view};desktopWindows.add(product);
  window.contentView.addChildView(view);
  const resize=()=>{const [width,height]=window.getContentSize();view.setBounds({x:0,y:0,width,height});};
  resize();window.contentView.on("bounds-changed",resize);
  window.on("close",event=>{
    if(!contents.isDestroyed()){event.preventDefault();contents.close({waitForBeforeUnload:true});}
  });
  contents.once("destroyed",()=>{if(!window.isDestroyed())window.destroy();});
  window.once("closed",()=>{desktopWindows.delete(product);if(!contents.isDestroyed())contents.close({waitForBeforeUnload:false});});
  return product;
}
