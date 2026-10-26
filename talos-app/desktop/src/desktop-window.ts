import { BaseWindow, WebContentsView, type WebContents, type BaseWindowConstructorOptions, type WebPreferences } from "electron";

/** Trusted product surfaces, separate from managed website targets and input modals. */
export const desktopWindows = new Set<{window:BaseWindow;view:WebContentsView}>();
export function isDesktopRenderer(contents:WebContents):boolean {
  return [...desktopWindows].some(product=>product.view.webContents===contents) && !contents.isDestroyed();
}

/** Own the actual product view so websites can be ordered both above and below it. */
export function createDesktopWindow(options:BaseWindowConstructorOptions,webPreferences:WebPreferences) {
  const window=new BaseWindow(options),view=new WebContentsView({webPreferences});
  const product={window,view};desktopWindows.add(product);
  window.contentView.addChildView(view);
  const resize=()=>{const [width,height]=window.getContentSize();view.setBounds({x:0,y:0,width,height});};
  resize();window.on("resize",resize);
  window.on("close",event=>{
    if(!view.webContents.isDestroyed()){event.preventDefault();view.webContents.close({waitForBeforeUnload:true});}
  });
  view.webContents.once("destroyed",()=>{if(!window.isDestroyed())window.destroy();});
  window.once("closed",()=>{desktopWindows.delete(product);if(!view.webContents.isDestroyed())view.webContents.close({waitForBeforeUnload:false});});
  return product;
}
