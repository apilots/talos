import {expect,it} from "vitest";
import {release,trackInputs} from "./browser-input";

it("keeps a newer key press tracked when an older release acknowledgement arrives late",async()=>{
  let acknowledge!:()=>void;
  const pending=new Promise<void>(resolve=>{acknowledge=resolve;});
  const events:string[]=[];
  const send=trackInputs(async(_method,params={})=>{
    events.push(String(params.type));
    if(events.length===2)await pending;
    return {};
  });
  await send("Input.dispatchKeyEvent",{type:"keyDown",key:"Enter"});
  const previous=send("Input.dispatchKeyEvent",{type:"keyUp",key:"Enter"});
  await send("Input.dispatchKeyEvent",{type:"keyDown",key:"Enter"});
  acknowledge();await previous;await release(send);
  expect(events).toEqual(["keyDown","keyUp","keyDown","keyUp"]);
});
