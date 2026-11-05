// Check crawler-visible metadata and the actual public image without page JavaScript.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base=new URL(process.argv[2]);
const publicBase='https://codelovesme.github.io/code/';
const routes=['index.html','learn.html','playground.html','docs.html','compare.html','modules.html','execution.html','memory.html',...JSON.parse(readFileSync(new URL('lessons.json',import.meta.url))).filter(x=>x.ready).map(x=>x.route)];
const browser=await chromium.launch({headless:true,channel:'chromium'});
try{
 const context=await browser.newContext({javaScriptEnabled:false});const page=await context.newPage();
 for(const route of routes){
  const response=await page.goto(new URL(route,base).href,{waitUntil:'domcontentloaded'});assert.equal(response.status(),200);
  const canonical=publicBase+(route==='index.html'?'':route);
  assert.equal(await page.locator('link[rel="canonical"]').count(),1);
  assert.equal(await page.locator('link[rel="canonical"]').getAttribute('href'),canonical);
  const meta=async selector=>{const el=page.locator(selector);assert.equal(await el.count(),1,`${route} ${selector}`);return el.getAttribute('content')};
  const title=await page.title(),description=await meta('meta[name="description"]');
  assert.ok(title.length>12 && description.length>30);
  for(const name of ['og:title','twitter:title']) assert.equal(await meta(`meta[${name.startsWith('og:')?'property':'name'}="${name}"]`),title);
  assert.equal(await meta('meta[property="og:description"]'),description);
  assert.equal(await meta('meta[name="twitter:description"]'),description);
  assert.equal(await meta('meta[property="og:url"]'),canonical);
  assert.equal(await meta('meta[property="og:type"]'),'website');
  assert.equal(await meta('meta[name="twitter:card"]'),'summary_large_image');
  for(const name of ['og:image','og:image:secure_url','twitter:image']) assert.equal(await meta(`meta[${name.startsWith('og:')?'property':'name'}="${name}"]`),publicBase+'share-code.png');
  assert.equal(await meta('meta[property="og:image:type"]'),'image/png');
  assert.equal(await meta('meta[property="og:image:width"]'),'1200');
  assert.equal(await meta('meta[property="og:image:height"]'),'630');
  const alt=await meta('meta[property="og:image:alt"]');assert.match(alt,/Built for reuse/);
  assert.equal(await meta('meta[name="twitter:image:alt"]'),alt);
  if(route==='index.html') {assert.match(title,/reuse.*performance/i);assert.match(description,/native.*WebAssembly/i)}
  if(route==='learn.html') assert.match(title,/Learn Code/);
  if(route==='compare.html') {assert.match(title,/comparison/);assert.match(description,/v2.23.0/)}
 }
 const response=await context.request.get(new URL('share-code.png',base).href);assert.equal(response.status(),200);
 assert.match(response.headers()['content-type'],/^image\/png/);
 const bytes=await response.body();assert.deepEqual(bytes,readFileSync(new URL('share-code.png',import.meta.url)));
 assert.equal(bytes.subarray(0,8).toString('hex'),'89504e470d0a1a0a');
 assert.equal(bytes.readUInt32BE(16),1200);assert.equal(bytes.readUInt32BE(20),630);assert.ok(bytes.length<1_000_000);
 console.log(`${routes.length} crawler-visible page titles/descriptions, canonical/OG/social URLs and exact 1200×630 PNG/alt/type passed without JavaScript; no crawler-cache assumption`);
}finally{await browser.close()}
