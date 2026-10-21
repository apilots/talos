import assert from "node:assert/strict";
import { waitForExpression } from "./desktop-cdp.mjs";
import { evaluate } from "./desktop-smoke-evaluate.mjs";

const toggle = 'document.querySelector(`button[aria-controls="talos-sidebar"]`)';

/** Verify passive loads, keyboard focus and pointer drawer recovery separately. */
export async function verifySidebarFocus(cdp, capture) {
  assert(await evaluate(cdp, `${toggle} !== null && document.activeElement !== ${toggle}`), "Initial load focused the sidebar toggle.");
  await cdp.call("Page.reload");
  await waitForExpression(cdp, 'Boolean(document.querySelector("[data-smoke=chat-composer-input]"))', 30_000);
  assert(await evaluate(cdp, `${toggle} !== null && document.activeElement !== ${toggle}`), "Refresh focused the sidebar toggle.");
  const initiallyExpanded = await evaluate(cdp, `${toggle}.getAttribute("aria-expanded")==="true"`);
  await capture("sidebar-default.png");
  await cdp.call("Page.bringToFront");
  for (let step = 0; step < 32 && !await evaluate(cdp, `document.activeElement === ${toggle}`); step += 1) {
    for (const type of ["keyDown", "keyUp"]) {
      await cdp.call("Input.dispatchKeyEvent", { type, key: "Tab", code: "Tab", windowsVirtualKeyCode: 9, nativeVirtualKeyCode: 9 });
    }
  }
  assert(await evaluate(cdp, `(()=>{const button=${toggle};const style=getComputedStyle(button);return document.activeElement===button && button.matches(":focus-visible") && style.outlineStyle!=="none" && parseFloat(style.outlineWidth)>0 && style.outlineColor!=="rgba(0, 0, 0, 0)";})()`), "Keyboard navigation did not expose a visible sidebar focus indicator.");
  await capture("sidebar-focus.png");
  // Start the pointer journey independently: Chromium can retain :focus-visible
  // when a pointer clicks the same element that already owns keyboard focus.
  const background = await evaluate(cdp, '(()=>{const r=document.querySelector("main").getBoundingClientRect();return {x:r.x+10,y:r.y+10};})()');
  await pointerClick(cdp, background);
  assert(await evaluate(cdp, `document.activeElement!==${toggle}`), "Pointer journey did not leave the previous keyboard focus.");
  // Real pointer input changes focus modality; programmatic click/focus cannot
  // prove that mouse-driven drawer restoration avoids a keyboard-only outline.
  for (const expanded of [!initiallyExpanded, initiallyExpanded]) {
    const position = await evaluate(cdp, `(()=>{const r=${toggle}.getBoundingClientRect();return {x:r.x+r.width/2,y:r.y+r.height/2};})()`);
    await pointerClick(cdp, position);
    await waitForExpression(cdp, `${toggle}?.getAttribute("aria-expanded")==="${expanded}"`, 5_000);
    assert(await evaluate(cdp, `document.activeElement===${toggle} && !${toggle}.matches(":focus-visible")`), "Pointer drawer action lost focus or retained keyboard-only focus styling.");
  }
  await capture("sidebar-pointer.png");
  await evaluate(cdp, "document.activeElement.blur()");
}

async function pointerClick(cdp, position) {
  await cdp.call("Input.dispatchMouseEvent", { type: "mouseMoved", ...position });
  for (const type of ["mousePressed", "mouseReleased"]) {
    await cdp.call("Input.dispatchMouseEvent", { type, button: "left", clickCount: 1, ...position });
  }
}
