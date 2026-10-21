import path from 'node:path';
import {createRequire} from 'node:module';
import {fileURLToPath,pathToFileURL} from 'node:url';
import {writeFile,cp} from 'node:fs/promises';
const directory=path.dirname(fileURLToPath(import.meta.url)),root=path.resolve(directory,'../../..');
const require=createRequire(path.join(root,'talos-app/desktop/package.json'));
const {build}=await import(pathToFileURL(require.resolve('vite')).href);
const selected=new Set(process.argv.slice(2));
for(const mode of selected)if(!['engine','mcp','media','viewer'].includes(mode))throw new Error(`Unknown native fixture mode: ${mode}`);
for(const [mode,output,main] of [['engine','dist','acceptance.mjs'],['mcp','mcp-dist','mcp.mjs'],['media','media-dist','media.mjs'],['viewer']]) {
 if(selected.size&&!selected.has(mode))continue;
 await build({configFile:path.join(directory,'vite.config.mjs'),mode});
 if(output)await cp(path.join(root,'talos-app/desktop/dist/browser-window'),path.join(process.env.TALOS_E2E_NATIVE_ROOT || path.join(root,'.tmp/browser-worker/native'),output,'browser-window'),{recursive:true});
 if(output)await writeFile(path.join(process.env.TALOS_E2E_NATIVE_ROOT || path.join(root,'.tmp/browser-worker/native'),output,'package.json'),JSON.stringify({name:`talos-browser-${mode}-acceptance`,version:'1.0.0',type:'module',main}));
}
