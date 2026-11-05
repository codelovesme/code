// Render the native brand/typography source; no external fonts or image service.
import assert from 'node:assert/strict';
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const browser=await chromium.launch({headless:true,channel:'chromium'});
try {
 const page=await browser.newPage({viewport:{width:1200,height:630},deviceScaleFactor:1});
 await page.goto(new URL('share-card/index.html',import.meta.url).href);
 await page.locator('img').evaluate(el=>el.decode());
 assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth),1200);
 for(const el of await page.locator('h1,.modes,footer').all()) assert.ok(await el.evaluate(el=>el.scrollWidth<=el.clientWidth));
 await page.screenshot({path:new URL('share-code.png',import.meta.url).pathname});
 await page.setViewportSize({width:600,height:315});
 await page.addStyleTag({content:'body {transform:scale(.5);transform-origin:top left}'});
 await page.screenshot({path:'/var/tmp/code-225-share-feed.png'});
 console.log('Rendered 1200×630 Code share card and 600×315 review preview');
}finally{await browser.close()}
