import { expect, it } from "vitest";
import { layoutWidths, MAIN_MIN_WIDTH } from "./layout-sizing";
it.each([1024,1280,1440,1920])("preserves main content space at viewport %i with both panes open",viewport=>{
 const available=viewport-56;
 for(const sidebar of [224,296,480])for(const workspace of [null,360,1040]){
  const result=layoutWidths(available,true,true,sidebar,workspace);
  expect(result.sidebar).toBeGreaterThanOrEqual(224);expect(result.workspace).toBeGreaterThanOrEqual(360);
  expect(available-result.sidebar-result.workspace).toBeGreaterThanOrEqual(MAIN_MIN_WIDTH);
 }
});
it("clamps the 1024px layout instead of squeezing the composer or overflowing",()=>{
 const result=layoutWidths(968,true,true,296,null);
 expect(result.sidebar).toBe(288);expect(result.workspace).toBe(360);
 expect(layoutWidths(1384,true,true,340,600).sidebar).toBe(340);
});

it("opens an unconfigured workspace compactly and preserves the main conversation area",()=>{
 const available=1440-56;
 const result=layoutWidths(available,true,true,296,null);
 expect(result.workspace).toBe(480);
 expect(available-result.sidebar-result.workspace).toBeGreaterThan(result.workspace);
 expect(layoutWidths(1920-56,false,true,296,null).workspace).toBe(480);
});
it("preserves a user-resized workspace instead of applying the opening default again",()=>{
 expect(layoutWidths(1560-56,true,true,296,700).workspace).toBe(700);
 expect(layoutWidths(1560-56,false,true,296,700).workspace).toBe(700);
 expect(layoutWidths(1024-56,true,true,296,null).workspace).toBe(360);
});
