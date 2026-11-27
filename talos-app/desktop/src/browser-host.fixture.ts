export function parseBrowserHostCommand(text: string): { id: number; method: string; params: Record<string, unknown> } {
  const value: unknown = JSON.parse(text);
  if (!value || typeof value !== "object" || !("id" in value) || typeof value.id !== "number"
    || !("method" in value) || typeof value.method !== "string") throw new Error("Invalid fixture CDP command.");
  const params: unknown = "params" in value ? value.params : {};
  if (!params || typeof params !== "object" || Array.isArray(params)) throw new Error("Invalid fixture CDP parameters.");
  return { id: value.id, method: value.method, params: params as Record<string, unknown> };
}

export class BrowserHostSocket extends EventTarget {
  static OPEN=1;
  static profileRoot="";
  static all:BrowserHostSocket[]=[];
  static targets:Array<{targetId:string;type:string;url:string;title:string}>=[];
  readyState=0;
  commands:Array<{id:number;method:string;params?:Record<string,unknown>}>=[];
  constructor(){super();BrowserHostSocket.all.push(this);queueMicrotask(()=>{this.readyState=1;this.dispatchEvent(new Event("open"));});}
  send(text:string){
    const command=parseBrowserHostCommand(text);this.commands.push(command);
    const result=command.method==="Browser.getBrowserCommandLine"?{arguments:["--enable-automation",`--user-data-dir=${BrowserHostSocket.profileRoot}`]}:command.method==="Target.getTargets"?{targetInfos:BrowserHostSocket.targets}:{};
    queueMicrotask(()=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,result})})));
  }
  close(){this.readyState=3;this.dispatchEvent(new Event("close"));}
}
