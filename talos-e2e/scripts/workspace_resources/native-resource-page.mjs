// Reuse the Desktop acceptance CDP transport for Electron's isolated partitions,
// which are deliberately not exposed as Playwright default-context pages.
import {writeFile} from 'node:fs/promises';
import {connectCdp} from '../../../talos-app/desktop/scripts/desktop-cdp.mjs';
import {evaluate} from '../../../talos-app/desktop/scripts/desktop-smoke-evaluate.mjs';
const [url,kind,value]=process.argv.slice(2);
const cdp=await connectCdp(url);
try {
 if(kind==='fill') {
  const {root}=await cdp.call('DOM.getDocument');
  const {nodeId}=await cdp.call('DOM.querySelector',{nodeId:root.nodeId,selector:'input[aria-label="Draft"]'});
  if(!nodeId)throw Error('The native fixture Draft input is missing');
  await cdp.call('DOM.focus',{nodeId});await cdp.call('Input.insertText',{text:value});
 } else if(kind==='screenshot') {
  const screenshot=await cdp.call('Page.captureScreenshot',{format:'png'});
  await writeFile(value,Buffer.from(screenshot.data,'base64'));
 } else if(kind!=='read')throw Error('Unsupported native resource acceptance action');
 process.stdout.write(JSON.stringify(await evaluate(cdp,'({value:document.querySelector("input[aria-label=Draft]")?.value,bridge:typeof window.talos,width:innerWidth,height:innerHeight})'))+'\n');
}finally{cdp.close();}
