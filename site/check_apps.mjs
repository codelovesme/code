// Home application previews: real assets, accessible details and bounded loading.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2]);
const browser = await chromium.launch({headless:true, channel:'chromium'});
try {
  const context = await browser.newContext({reducedMotion:'reduce'});
  const page = await context.newPage();
  const errors=[], missing=[], forbidden=[];
  let previewBytes=0;
  page.on('pageerror', e=>errors.push(e.message));
  page.on('response', r=>{if(r.status()>=400) missing.push(r.url());});
  page.on('request', r=>{if(/\.wasm|\/pkg\/|aqua\.codeloves\.me|apps\.codeloves\.me/.test(r.url())) forbidden.push(r.url());});
  await page.goto(new URL('index.html#built-with-code',base).href,{waitUntil:'networkidle'});
  const section=page.locator('#built-with-code');
  assert.equal(await section.locator('article').count(),2);
  assert.equal(await section.locator('iframe, video, audio, canvas').count(),0);
  for (const [name,width,height] of [['aquarium',800,500],['tictac',600,471]]) {
    const img=section.locator(`img[src="app-${name}.png"]`);
    await img.scrollIntoViewIfNeeded();
    await img.evaluate(el=>el.decode());
    assert.equal(await img.getAttribute('loading'),'lazy');
    assert.ok((await img.getAttribute('alt')).length>25);
    assert.deepEqual(await img.evaluate(el=>[el.naturalWidth,el.naturalHeight]),[width,height]);
    const response=await page.request.get(new URL(`app-${name}.png`,base).href);
    assert.equal(response.status(),200);
    assert.deepEqual(await response.body(),readFileSync(new URL(`app-${name}.png`,import.meta.url)));
    previewBytes+=(await response.body()).length;
    const details=section.locator(`#${name}-details`);
    assert.equal(await details.evaluate(el=>el.open),false);
    await details.locator('summary').focus(); await page.keyboard.press('Enter');
    assert.equal(await details.evaluate(el=>el.open),true);
    assert.ok((await details.innerText()).includes('Euglena'));
    await page.keyboard.press('Enter');
    assert.equal(await details.evaluate(el=>el.open),false);
  }
  assert.ok(previewBytes<200_000);
  assert.equal(await section.getByRole('link',{name:'Open Aquarium ↗',exact:true}).getAttribute('href'),'https://aqua.codeloves.me');
  assert.equal(await section.getByRole('link',{name:'Play Tic-Tac-Toe ↗',exact:true}).getAttribute('href'),'https://apps.codeloves.me/tictac');
  assert.equal(await section.getByRole('link',{name:'Euglena ↗',exact:true}).getAttribute('href'),'https://github.com/codelovesme/euglena');
  assert.ok((await section.locator('article').first().innerText()).includes('Sign-in required'));
  assert.ok((await section.locator('article').last().innerText()).includes('No account needed'));
  for (const width of [360,390,768,1440]) {
    await page.setViewportSize({width,height:900});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`${width}px overflow`);
    const boxes=[];
    for (const card of await section.locator('article').all()) {
      const box=await card.boundingBox();boxes.push(await card.evaluate(el=>{const r=el.getBoundingClientRect();return {x:r.x+scrollX,y:r.y+scrollY};}));
      assert.ok(box.x>=0 && box.x+box.width<=width+1);
      for(const link of await card.getByRole('link').all()) assert.equal(await link.isVisible(),true);
      await card.locator('summary').click();
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
      await card.locator('summary').click();
    }
    assert.equal(width<=700 ? boxes[1].y>boxes[0].y : boxes[1].x>boxes[0].x,true);
    if(width===390 || width===1440) {
      await section.scrollIntoViewIfNeeded();
      await section.screenshot({style:'.site-nav, .skip-link { visibility: hidden !important; }', path:`/var/tmp/code-223-showcase-${width}.png`});
    }
  }
  assert.deepEqual(errors,[]);assert.deepEqual(missing,[]);assert.deepEqual(forbidden,[]);
  const plainContext=await browser.newContext({javaScriptEnabled:false,viewport:{width:390,height:844}});
  const plain=await plainContext.newPage();
  await plain.goto(new URL('index.html#built-with-code',base).href);
  // Let the native fragment's smooth scroll settle before pointer automation.
  await plain.waitForTimeout(1000);
  await plain.locator('#aquarium-details summary').click();
  assert.ok((await plain.locator('#aquarium-details').innerText()).includes('standalone'));
  await plain.locator('#tictac-details summary').scrollIntoViewIfNeeded();
  await plain.waitForTimeout(1000);
  await plain.locator('#tictac-details summary').click();
  assert.ok((await plain.locator('#tictac-details').innerText()).includes('storage'));
  assert.equal(await plain.locator('#built-with-code a').count(),3);
  await plainContext.close();
  console.log('Home apps: two canonical lazy previews, alt text/dimensions/budget, real links, keyboard/no-JS details and four widths passed; no app/engine/media loaded on Home');
} finally {await browser.close();}
