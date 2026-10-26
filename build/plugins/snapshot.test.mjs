import {test} from 'node:test';
import assert from 'node:assert/strict';
import {execFileSync} from 'node:child_process';
import {existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {snapshot} from './snapshot.mjs';

test('fixed commit snapshot replaces stale content and a failed fetch preserves it', () => {
  const root=mkdtempSync(path.join(os.tmpdir(),'talos-mcp-snapshot-'));
  const repo=path.join(root,'repo'), output=path.join(root,'output');
  mkdirSync(path.join(repo,'mcp/browser'),{recursive:true});
  const git=(...args)=>execFileSync('git',args,{cwd:repo,encoding:'utf8',stdio:['ignore','pipe','pipe']}).trim();
  try {
    git('init');git('config','user.name','Test');git('config','user.email','test@example.test');
    writeFileSync(path.join(repo,'mcp/browser/talos-mcp.json'),'first');git('add','.');git('commit','-m','first');
    const source={repository:repo,commit:git('rev-parse','HEAD')};
    writeFileSync(path.join(repo,'mcp/browser/talos-mcp.json'),'second');git('add','.');git('commit','-m','second');
    mkdirSync(output);writeFileSync(path.join(output,'stale'),'old tree');
    snapshot(output,source);
    assert.equal(readFileSync(path.join(output,'mcp/browser/talos-mcp.json'),'utf8'),'first');
    assert.deepEqual(JSON.parse(readFileSync(path.join(output,'source.json'),'utf8')),source);
    assert.equal(existsSync(path.join(output,'stale')),false);
    assert.throws(()=>snapshot(output,{...source,commit:'0'.repeat(40)}));
    assert.equal(readFileSync(path.join(output,'mcp/browser/talos-mcp.json'),'utf8'),'first');
  } finally {rmSync(root,{recursive:true,force:true});}
});
