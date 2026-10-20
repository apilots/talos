export type RecordValue = Record<string, unknown>;
export type Send = (method: string, params?: RecordValue) => Promise<RecordValue>;
const heldInputs=new WeakMap<Send,Map<string,{method:string;params:RecordValue}>>();
export function trackInputs(raw:Send):Send {
  const held=new Map<string,{method:string;params:RecordValue}>();
  const send:Send=async(method,params={})=>{
    const key=method==="Input.dispatchKeyEvent" ? `key:${String(params.key)}` : method==="Input.dispatchMouseEvent" ? `mouse:${String(params.button)}` : null;
    const down=params.type==="keyDown"||params.type==="rawKeyDown"||params.type==="mousePressed", up=params.type==="keyUp"||params.type==="mouseReleased";
    if(key&&down) {if(held.size>=256)throw new Error("Pressed input capacity exceeded.");held.set(key,{method,params:{...params,type:params.type==="mousePressed"?"mouseReleased":"keyUp",modifiers:0,buttons:0}});}
    const result=await raw(method,params);
    if(key&&up)held.delete(key);
    return result;
  };
  heldInputs.set(send,held);return send;
}
export async function release(send:Send):Promise<void> {
  const held=heldInputs.get(send);
  if(held)for(const {method,params} of [...held.values()])await send(method,params);
}

