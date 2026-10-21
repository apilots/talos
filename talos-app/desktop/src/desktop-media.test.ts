import {afterEach,describe,expect,it,vi} from 'vitest';
import {EventEmitter} from 'node:events';
const electron=vi.hoisted(()=>({fromWebContents:vi.fn(()=>({}))}));
vi.mock('electron',()=>({BrowserWindow:{fromWebContents:electron.fromWebContents}}));
import {desktopMediaHost} from './desktop-media';
import type {WebContents} from 'electron';
const owner=()=>Object.assign(new EventEmitter(),{send:vi.fn(),isDestroyed:()=>false}) as unknown as WebContents;
afterEach(()=>{desktopMediaHost.reset();vi.useRealTimers();});
describe('Desktop media frame ownership and backpressure',()=>{
 it('waits for consumption by the exact opening renderer',async()=>{
  const renderer=owner(),other=owner();desktopMediaHost.own('stream',renderer);
  let done=false;const pending=desktopMediaHost.frame('request',{streamId:'stream',dataBase64:'UkZC'}).then(()=>{done=true;});
  expect(renderer.send).toHaveBeenCalledOnce();expect(done).toBe(false);
  expect(()=>desktopMediaHost.acknowledge('request',other)).toThrow();
  desktopMediaHost.acknowledge('request',renderer);await pending;expect(done).toBe(true);
  expect(()=>desktopMediaHost.acknowledge('request',renderer)).toThrow();
 });
 it('terminates a stalled renderer and rejects missing stream owners',async()=>{
  vi.useFakeTimers();const renderer=owner();desktopMediaHost.own('stream',renderer);
  const pending=desktopMediaHost.frame('request',{streamId:'stream',dataBase64:'UkZC'});
  const failure=expect(pending).rejects.toThrow('stopped consuming');
  await vi.advanceTimersByTimeAsync(1801);await failure;
  desktopMediaHost.own('replacement-1',renderer);desktopMediaHost.own('replacement-2',renderer);
  await expect(desktopMediaHost.frame('other',{streamId:'unknown',dataBase64:'UkZC'})).rejects.toThrow();
 });
 it('releases both backend streams and pending frames when the opening window is destroyed',async()=>{
  const renderer=owner(),stopA=vi.fn(),stopB=vi.fn();
  desktopMediaHost.own('a',renderer,stopA);desktopMediaHost.own('b',renderer,stopB);
  const pending=desktopMediaHost.frame('request',{streamId:'a',dataBase64:'UkZC'});
  const failure=expect(pending).rejects.toThrow('stream closed');
  renderer.emit('destroyed');await failure;
  expect(stopA).toHaveBeenCalledOnce();expect(stopB).toHaveBeenCalledOnce();
  expect(renderer.listenerCount('did-start-navigation')).toBe(0);
  desktopMediaHost.own('new-a',owner());desktopMediaHost.own('new-b',owner());
 });
 it('keeps same-document routing but closes a replaced renderer document',async()=>{
  const renderer=owner(),stop=vi.fn();desktopMediaHost.own('stream',renderer,stop);
  renderer.emit('did-start-navigation',{isMainFrame:true,isSameDocument:true});expect(stop).not.toHaveBeenCalled();
  const pending=desktopMediaHost.frame('request',{streamId:'stream',dataBase64:'UkZC'});
  const failure=expect(pending).rejects.toThrow('stream closed');
  renderer.emit('did-start-navigation',{isMainFrame:true,isSameDocument:false});await failure;
  expect(stop).toHaveBeenCalledOnce();desktopMediaHost.own('new-stream',renderer);
 });
});
