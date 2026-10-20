import path from 'node:path';
import {builtinModules} from 'node:module';
import {fileURLToPath} from 'node:url';
const directory=path.dirname(fileURLToPath(import.meta.url));
const root=path.resolve(directory,'../../..');
export default ({mode})=>{
 if(mode==='viewer')return {root:path.join(directory,'viewer'),base:'./',resolve:{alias:{'@novnc/novnc':path.join(root,'talos-app/web/node_modules/@novnc/novnc')}},build:{outDir:path.join(root,'.tmp/browser-worker/native/viewer-dist'),emptyOutDir:true}};
 const entries={engine:['engine.ts','dist','acceptance.mjs'],mcp:['native-mcp.ts','mcp-dist','mcp.mjs'],media:['native-media.ts','media-dist','media.mjs']};
 const entry=entries[mode];if(!entry)throw new Error('Select engine, mcp, media or viewer fixture mode.');
 return {root,build:{target:'node22',outDir:path.join(root,'.tmp/browser-worker/native',entry[1]),emptyOutDir:true,lib:{entry:path.join(directory,entry[0]),formats:['es'],fileName:()=>entry[2]},rollupOptions:{external:['electron',...builtinModules,...builtinModules.map(name=>'node:'+name)]}}};
};
