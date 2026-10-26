// Build-time source snapshot only; installation remains owned by the target Worker.
import {execFileSync} from 'node:child_process';
import {cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

export function snapshot(output, source) {
  if (!source.repository || !/^[a-f0-9]{40}$/.test(source.commit)) throw new Error('An immutable MCP source commit is required');
  output = path.resolve(output);
  mkdirSync(path.dirname(output), {recursive:true});
  const staging = mkdtempSync(path.join(path.dirname(output), '.mcps-'));
  const git = (...args) => execFileSync('git', args, {cwd:staging, encoding:'utf8', timeout:120000, stdio:['ignore','pipe','pipe']}).trim();
  try {
    git('init');
    git('fetch', '--depth=1', source.repository, source.commit);
    git('checkout', '--detach', 'FETCH_HEAD');
    if (git('rev-parse', 'HEAD') !== source.commit) throw new Error('MCP source revision mismatch');
    function verify(directory) {
      for (const name of readdirSync(directory)) {
        const file = path.join(directory, name), info = lstatSync(file);
        if (info.isSymbolicLink()) throw new Error('MCP snapshots must not contain symbolic links');
        if (info.isDirectory()) verify(file);
      }
    }
    verify(path.join(staging, 'mcp'));
    const result = path.join(staging, 'snapshot');
    mkdirSync(result);
    cpSync(path.join(staging, 'mcp'), path.join(result, 'mcp'), {recursive:true});
    writeFileSync(path.join(result, 'source.json'), JSON.stringify(source, null, 2) + '\n');
    const previous = path.join(staging, 'previous');
    if (existsSync(output)) renameSync(output, previous);
    try { renameSync(result, output); }
    catch (error) { if (existsSync(previous)) renameSync(previous, output); throw error; }
  } finally { rmSync(staging, {recursive:true, force:true}); }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const output = process.argv[2];
  if (!output) throw new Error('Usage: node build/plugins/snapshot.mjs <output>');
  snapshot(output, JSON.parse(readFileSync(new URL('./official-mcp.json', import.meta.url), 'utf8')));
}
