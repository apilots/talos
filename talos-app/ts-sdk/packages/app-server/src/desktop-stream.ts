import type {AppProtocol} from '@talos/sdk-core';
import {unwrapTalosApiResponse} from '@talos/sdk-core';
import type {DesktopRendererBridge} from './desktop';

/** noVNC raw channel over the protected Desktop bridge; no native public listener. */
class DesktopStreamChannel extends EventTarget {
  binaryType:BinaryType='arraybuffer';
  protocol='';
  readyState=0;
  onopen:((event:Event)=>void)|null=null;
  onclose:((event:Event)=>void)|null=null;
  onerror:((event:Event)=>void)|null=null;
  onmessage:((event:MessageEvent)=>void)|null=null;
  private queued=0;
  private writing=Promise.resolve();
  private readonly removeFrame:()=>void;
  private readonly removeState:()=>void;
  private early:AppProtocol.DesktopMediaFrame|null=null;
  constructor(private readonly bridge:DesktopRendererBridge,private readonly grant:AppProtocol.DesktopViewGrant,private readonly options:{actorCredentials:AppProtocol.ActorCredentials|null}) {
    super();
    if(!bridge.onDesktopFrame)throw new Error('Desktop media bridge is unavailable.');
    this.removeFrame=bridge.onDesktopFrame(frame=>{
      if(frame.streamId!==grant.streamId)return;
      if(this.readyState===0){if(this.early)this.fail();else this.early=frame;return;}
      this.receive(frame);
    });
    this.removeState=bridge.onRuntimeState?.(state=>{if(state.kind!=='ready')this.fail();})??(()=>{});
    void bridge.invoke<null>('desktop/streamOpen',{streamId:grant.streamId,ticket:grant.ticket},options).then(response=>{
      unwrapTalosApiResponse<null>(response);
      if(this.readyState!==0)return;
      this.readyState=1;const event=new Event('open');this.onopen?.(event);this.dispatchEvent(event);
      if(this.early){const frame=this.early;this.early=null;this.receive(frame);}
    }).catch(error=>this.fail(error));
  }
  private receive(frame:AppProtocol.DesktopMediaFrame):void {
    if(this.readyState!==1)return;
    if(frame.dataBase64===null){this.close();return;}
    if(frame.dataBase64.length>88*1024){this.fail();return;}
    let bytes:Uint8Array;
    try {bytes=Uint8Array.from(atob(frame.dataBase64),character=>character.charCodeAt(0));}
    catch(error){this.fail(error);return;}
    if(bytes.length>64*1024){this.fail();return;}
    const event=new MessageEvent('message',{data:bytes.buffer});this.onmessage?.(event);this.dispatchEvent(event);
  }
  send(data:string|ArrayBuffer|ArrayBufferView):void {
    if(this.readyState!==1)throw new Error('Desktop media channel is not open.');
    const bytes=typeof data==='string'?new TextEncoder().encode(data):data instanceof ArrayBuffer?new Uint8Array(data):new Uint8Array(data.buffer,data.byteOffset,data.byteLength);
    if(this.queued+bytes.length>4*1024*1024){this.fail();return;}
    this.queued+=bytes.length;
    for(let offset=0;offset<bytes.length;offset+=64*1024) {
      const chunk=bytes.slice(offset,offset+64*1024);
      this.writing=this.writing.then(async()=>{
        try {
          if(this.readyState!==1)return;
          const response=await this.bridge.invoke<null>('desktop/streamSend',{streamId:this.grant.streamId,dataBase64:btoa(String.fromCharCode(...chunk))},this.options);
          unwrapTalosApiResponse<null>(response);
        }finally{this.queued-=chunk.length;}
      }).catch(error=>this.fail(error));
    }
  }
  close():void {
    if(this.readyState===3)return;
    this.readyState=3;this.removeFrame();this.removeState();this.early=null;
    void this.bridge.invoke<null>('desktop/streamClose',{streamId:this.grant.streamId},this.options).catch(()=>{});
    const event=new CloseEvent('close',{code:1000,wasClean:true});this.onclose?.(event);this.dispatchEvent(event);
  }
  private fail(cause?:unknown):void {if(this.readyState===3)return;const event=new ErrorEvent('error',{error:cause});this.onerror?.(event);this.dispatchEvent(event);this.close();}
}
export function createNativeDesktopStream(bridge:DesktopRendererBridge,grant:AppProtocol.DesktopViewGrant,actorCredentials:AppProtocol.ActorCredentials|null):WebSocket {
  return new DesktopStreamChannel(bridge,grant,{actorCredentials}) as unknown as WebSocket;
}
