export class BrowserHostSocket extends EventTarget {
  static OPEN=1;
  static profileRoot="";
  static all:BrowserHostSocket[]=[];
  static targets:Array<{targetId:string;type:string;url:string;title:string}>=[];
  readyState=0;
  commands:Array<{id:number;method:string;params?:Record<string,unknown>}>=[];
  constructor(){super();BrowserHostSocket.all.push(this);queueMicrotask(()=>{this.readyState=1;this.dispatchEvent(new Event("open"));});}
  send(text:string){
    const command=JSON.parse(text);this.commands.push(command);
    const result=command.method==="Browser.getBrowserCommandLine"?{arguments:["--enable-automation",`--user-data-dir=${BrowserHostSocket.profileRoot}`]}:command.method==="Target.getTargets"?{targetInfos:BrowserHostSocket.targets}:{};
    queueMicrotask(()=>this.dispatchEvent(new MessageEvent("message",{data:JSON.stringify({id:command.id,result})})));
  }
  close(){this.readyState=3;this.dispatchEvent(new Event("close"));}
}
