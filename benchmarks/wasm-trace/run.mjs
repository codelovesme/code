// node benchmarks/wasm-trace/run.mjs target/debug/code [baseline-host.mjs]
// Release wasm, alternating hosts, two warmups then 11 measured samples.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { pathToFileURL } from 'node:url';
const compiler = resolve(process.argv[2] ?? 'target/debug/code');
const dir = mkdtempSync(join(tmpdir(), 'code-bench-wasm-trace-'));
try {
  const logic = 'Tick { n } =>\n    emit Next { n = n } to this get result\n    return result\nNext { n } => return Value { value = n + 1 }\n';
  for (const name of ['quiet', 'traced']) {
    writeFileSync(join(dir, name + '.code'), (name === 'traced' ? 'Trace => return Recorded\n' : '') + logic);
    execFileSync(compiler, ['build', join(dir,name+'.code'), '--release', '--target', 'wasm', '-o', join(dir,name+'.wasm')]);
  }
  const { createHost } = await import(pathToFileURL(join(dir,'host.mjs')));
  const variants = [['quiet', createHost, 'quiet'], ['traced', createHost, 'traced']];
  if (process.argv[3]) {
    const baseline = await import(pathToFileURL(resolve(process.argv[3])));
    variants.unshift(['baseline quiet host', baseline.createHost, 'quiet']);
  }
  const hosts = [];
  for (const [name, create, file] of variants) {
    const host = create();
    const { instance } = await WebAssembly.instantiate(readFileSync(join(dir,file+'.wasm')), {env:host.env});
    host.start(instance); hosts.push({name,host,samples:[]});
  }
  for (let round=0; round<13; round++) for (let offset=0; offset<hosts.length; offset++) {
    const item = hosts[(round+offset)%hosts.length];
    const start = performance.now();
    for (let i=0; i<10000; i++) if (item.host.ask({_class:'Tick',n:i}).value !== i+1) throw Error('wrong answer');
    if (round>=2) item.samples.push(performance.now()-start);
  }
  const results = hosts.map(({name,host,samples})=>{
    host.stop();
    return {name,medianMs:[...samples].sort((a,b)=>a-b)[5],samples};
  });
  console.log(JSON.stringify({node:process.version,compiler:execFileSync(compiler,['--version'],{encoding:'utf8'}).trim(), release:true, warmups:2,runs:11,asksPerRun:10000,results},null,2));
} finally { rmSync(dir,{recursive:true,force:true}); }
