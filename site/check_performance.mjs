// Chart-led Comparison: checked values/scales, same-page detail and responsive access.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base=new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const config=JSON.parse(readFileSync(new URL('comparison-data.json',import.meta.url)));
const report=JSON.parse(readFileSync(new URL('../'+config.comparison,import.meta.url)));
const summaries=new Map(report.summary.map(r=>[[r.group,r.workload,r.mode].join('|'),r]));
const pilot=JSON.parse(readFileSync(new URL('../'+config.history,import.meta.url)));
for(const row of pilot.summary)summaries.set(['history',row.workload,row.mode].join('|'),row);
const browser=await chromium.launch({headless:true,channel:'chromium'});
function expected(key){const parts=key.split('|');const [metric,stat='median']=parts.pop().split('-');const row=summaries.get(parts.join('|'));assert.ok(row,key);return metric==='time'?row[`${stat}_seconds`]*1000:row[`${stat}_peak_rss_kib`]/1024}
try {
 const c=await browser.newContext({reducedMotion:'reduce'});const p=await c.newPage();
 const errors=[],engines=[];p.on('pageerror',e=>errors.push(e.message));p.on('request',r=>{if(/\.wasm|\/pkg\//.test(r.url()))engines.push(r.url())});
 await p.goto(new URL('index.html',base).href,{waitUntil:'networkidle'});
 await p.locator('.site-nav').getByRole('link',{name:'Comparison',exact:true}).click();
 await p.waitForURL('**/compare.html');assert.equal(await p.locator('.site-nav [aria-current="page"]').innerText(),'Comparison');
 assert.equal(await p.locator('.comparison-chart').count(),7);assert.equal(await p.locator('details[open]').count(),0);
 const expectedRows={'compiled-runtime':4,'source-runtime':5,'compile':4,'memory':7,'snapshot-memory':5,'configuration-runtime':5,'configuration-memory':5};
 for(const chart of await p.locator('.comparison-chart').all()){
  const name=await chart.getAttribute('data-chart'),scale=Number(await chart.getAttribute('data-scale-max'));
  assert.equal(await chart.locator('.comparison-bar-row').count(),expectedRows[name]);
  let previous=-Infinity;
  for(const row of await chart.locator('.comparison-bar-row').all()){
   const key=await row.locator('[data-evidence]').getAttribute('data-evidence');const amount=expected(key);
   assert.ok(amount>=previous);previous=amount;assert.ok(amount<=scale);
   const width=parseFloat((await row.locator('.perf-bar').getAttribute('style')).match(/width:([\d.]+)%/)[1]);
   assert.ok(Math.abs(width-amount/scale*100)<0.000002,`${name}: inaccurate bar ${key}`);
  }
 }
 for(const span of await p.locator('[data-evidence]').all()){const key=await span.getAttribute('data-evidence');assert.ok(Math.abs(Number(await span.textContent())-expected(key))<=0.000500001,`rounding/value mismatch: ${key}`)}
 assert.equal(await p.locator('.comparison-all tbody tr').count(),35);
 assert.equal(await p.locator('.history-all tbody tr').count(),20);
 const documentRatio=summaries.get('history|document_1m|Code automatic').median_seconds/summaries.get('history|document_1m|Rust shared').median_seconds;
 assert.ok((await p.locator('#sharing').innerText()).includes(`${documentRatio.toFixed(2)}× the time`));
 assert.match(await p.locator('#sharing').innerText(),/Same measured VM/);
 const broadOrder=await p.locator('#broader-native-results tbody tr').evaluateAll(rows=>rows.map(row=>row.querySelector('[data-evidence]').dataset.evidence.split('|')[1]));
 const ratio=workload=>summaries.get(`prebuilt|${workload}|Code release`).median_seconds/Math.min(...['C O2','Rust O3'].map(mode=>summaries.get(`prebuilt|${workload}|${mode}`).median_seconds));
 const workloads=['binary_trees','dictionary','points','spectral_norm','string_build'];
 assert.deepEqual(broadOrder,workloads.sort((a,b)=>ratio(a)-ratio(b)));
 assert.ok((await p.locator('#performance .comparison-context').innerText()).includes('strongest first'));
 assert.ok((await p.locator('#execution').innerText()).includes('Cached Code executes native code'));
 assert.ok(!(await p.locator('main').innerText()).includes('Four ways to run'));
 const old=['at-a-glance','matrix-title','performance','compiled-runtime-chart-title','source-runtime-chart-title','compile-chart-title','memory','memory-chart-title','types-title','structure-title','errors-title','concurrency-title','fit','sources-title'];
 for(const id of old) assert.equal(await p.locator(`[id="${id}"]`).count(),1,id);
 for(const width of [360,390,768,1440]){
  await p.setViewportSize({width,height:900});
  for(const chart of await p.locator('.comparison-chart').all()){
   assert.equal(await chart.isVisible(),true);assert.equal(await chart.locator('..').evaluate(el=>el.closest('details')!==null),false);
   const box=await chart.boundingBox();assert.ok(box.x>=0 && box.x+box.width<=width+1);
  }
  for(const s of await p.locator('summary').all()){await s.focus();await p.keyboard.press('Enter');assert.equal(await s.evaluate(el=>el.parentElement.open),true)}
  assert.ok((await p.locator('main').innerText()).includes('slower than Rust in all five'));
  assert.ok((await p.locator('#snapshots').innerText()).includes('Explicit C/Rust copy-on-write is faster and leaner'));
  assert.equal(await p.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`${width}px overflow`);
  for(const table of await p.locator('table').all()){const box=await table.boundingBox();assert.ok(box.x>=0 && box.x+box.width<=width+1,`table at ${width}px`)}
  for(const s of await p.locator('summary').all()){await s.focus();await p.keyboard.press('Space');assert.equal(await s.evaluate(el=>el.parentElement.open),false)}
 }
 for(const anchor of ['performance','execution','compilation','memory','snapshots','sharing','full-results','methodology']){
  await p.goto(new URL(`compare.html#${anchor}`,base).href,{waitUntil:'networkidle'});assert.equal(await p.locator(`#${anchor}`).isVisible(),true);
 }
 for(const width of [390,1440]){await p.setViewportSize({width,height:900});await p.evaluate(()=>scrollTo(0,0));await p.screenshot({path:`/var/tmp/code-229-comparison-${width}.png`,fullPage:true})}
 const sourceLinks=await p.locator('a[href*="github.com/codelovesme/code/blob/"]').evaluateAll(a=>a.map(el=>el.href));assert.ok(sourceLinks.length>=6);assert.ok(sourceLinks.every(x=>x.includes('/'+config.evidence_commit+'/') || x.includes('/65403bd/') || x.includes('/8d0656a/')));
 assert.deepEqual(errors,[]);assert.deepEqual(engines,[]);
 const plain=await browser.newContext({javaScriptEnabled:false,reducedMotion:'reduce'});const q=await plain.newPage();await q.goto(new URL('compare.html',base).href);assert.equal(await q.locator('.comparison-chart').count(),7);await q.locator('#full-results summary').click();assert.equal(await q.locator('.comparison-all tbody tr').count(),35);await q.getByText('All 20 sharing-study results · time and memory',{exact:true}).click();assert.equal(await q.locator('.history-all').isVisible(),true);assert.equal(await q.locator('.history-all tbody tr').count(),20);await plain.close();
 console.log('Comparison: seven visible ranked charts/35 bars, every median/range/linear scale, all 35 original and 20 separate pilot results, 14 legacy targets, keyboard/no-JS and four widths passed; no engine loaded');
}finally{await browser.close()}
