// Shared mobile/accessibility audit. AXE_PATH must point to axe-core 4.14.0/axe.min.js.
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync} from 'node:fs';
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
if(!process.env.AXE_PATH) throw Error('Set AXE_PATH to the pinned axe-core 4.14.0/axe.min.js');
const axeSource=readFileSync(process.env.AXE_PATH,'utf8');
const base=new URL(process.argv[2]);
const url=path=>new URL(path,base).href;
const routes=['index.html','learn.html','playground.html','docs.html','compare.html','modules.html','execution.html','memory.html',...JSON.parse(readFileSync(new URL('lessons.json',import.meta.url))).filter(x=>x.ready).map(x=>x.route)];
const browser=await chromium.launch({headless:true,channel:'chromium'});
const audit=[];
try {
  const context=await browser.newContext({reducedMotion:'reduce'});
  const page=await context.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  for(const route of routes){
    await page.goto(url(route),{waitUntil:'networkidle'});
    if(route==='playground.html') await page.locator('#run:enabled').waitFor();
    await page.locator('details').evaluateAll(els=>els.forEach(el=>el.open=true));
    if(route==='playground.html') await page.locator('.ex-card').first().waitFor();
    assert.equal(await page.locator('h1').count(),1,route);
    for(const width of [360,390,768,1440]){
      await page.setViewportSize({width,height:900});
      assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`${route} ${width}px overflow`);
      for(const card of await page.locator('.ex-card, .route-card, .home-app, .learning-path li').all()) {
        const b=await card.boundingBox();assert.ok(b.x>=0 && b.x+b.width<=width+1,`${route} card escapes ${width}px`);
      }
    }
    await page.addScriptTag({content:axeSource});
    assert.equal(await page.evaluate(()=>axe.version),'4.14.0');
    for(const width of [390,1440]){
      await page.setViewportSize({width,height:900});
      const result=await page.evaluate(async()=>{const r=await axe.run(document,{runOnly:{type:'tag',values:['wcag2a','wcag2aa','wcag21a','wcag21aa','wcag22aa','best-practice']}});return {violations:r.violations.map(v=>({id:v.id,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))})),incomplete:r.incomplete.map(v=>({id:v.id,count:v.nodes.length,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))}});
      audit.push({route,width,...result});
      assert.deepEqual(result.violations,[],`${route} ${width}px: ${JSON.stringify(result.violations)}`);
    }
  }
  // All fixture pages, including long names/code, stay inside the phone viewport.
  await page.setViewportSize({width:390,height:844});
  await page.goto(url('playground.html#examples'),{waitUntil:'networkidle'});
  let cards=0;
  while(true){
    for(const card of await page.locator('.ex-card').all()){
      const b=await card.boundingBox();assert.ok(b.x>=0 && b.x+b.width<=390);cards++;
    }
    if(await page.locator('#next-examples').isDisabled()) break;
    await page.locator('#next-examples').click();
  }
  const examples=await page.locator('#examples-data').evaluate(el=>JSON.parse(el.textContent));
  assert.equal(cards,examples.length);
  // Keyboard exit in both directions, result access and copy fallback.
  await page.goto(url('playground.html'),{waitUntil:'networkidle'});
  await page.locator('#run:enabled').waitFor();
  await page.locator('#source').focus();await page.keyboard.press('Shift+Tab');
  assert.notEqual(await page.evaluate(()=>document.activeElement.id),'source');
  await page.locator('#source').focus();await page.keyboard.press('Escape');
  assert.equal(await page.evaluate(()=>document.activeElement.id),'run');
  await page.getByRole('link',{name:'View result ↓',exact:true}).click();
  assert.equal(await page.evaluate(()=>document.activeElement.id),'output');
  await page.evaluate(()=>Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async()=>{throw Error('clipboard unavailable')}}}));
  await page.getByRole('button',{name:'Copy result',exact:true}).click();
  await page.locator('#result-copy-status').filter({hasText:'Result selected'}).waitFor();
  assert.equal(await page.evaluate(()=>getSelection().toString()),'hello, world');
  assert.equal(await page.evaluate(()=>document.activeElement.id),'output');
  assert.equal(await page.locator('#output').getAttribute('role'),'region');
  assert.equal(await page.locator('.blob').first().evaluate(el=>getComputedStyle(el).animationName),'none');
  // Text enlargement/reflow without masking card overflow.
  for(const route of ['index.html','learn.html','learn/small-program.html','playground.html']){
    await page.goto(url(route),{waitUntil:'networkidle'});
    await page.evaluate(()=>document.documentElement.style.fontSize='200%');
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true,`${route} 200% text overflow`);
  }
  // Readable static example when scripts are unavailable.
  const plainContext=await browser.newContext({javaScriptEnabled:false,reducedMotion:'reduce',viewport:{width:390,height:844}});
  const plain=await plainContext.newPage();await plain.goto(url('playground.html'));
  assert.match(await plain.locator('#source').inputValue(),/emit Print/);
  assert.equal(await plain.locator('#source').getAttribute('readonly'),'');
  assert.notEqual(await plain.locator('#source').evaluate(el=>getComputedStyle(el).color),'rgba(0, 0, 0, 0)');
  assert.equal(await plain.locator('#output').textContent(),'hello, world');
  assert.equal(await plain.locator('#run').isDisabled(),true);
  assert.ok(await plain.getByText('This is the original example and its expected result.',{exact:false}).isVisible());
  await plainContext.close();
  const forced=await browser.newContext({forcedColors:'active',viewport:{width:390,height:844}});
  const highContrast=await forced.newPage();await highContrast.goto(url('playground.html'),{waitUntil:'networkidle'});
  await highContrast.locator('#run:enabled').waitFor();
  const colors=await highContrast.locator('#source').evaluate(el=>{const s=getComputedStyle(el);return [s.color,s.backgroundColor]});
  assert.notEqual(colors[0],colors[1]);assert.notEqual(colors[0],'rgba(0, 0, 0, 0)');
  assert.equal(await highContrast.locator('.editor-wrap pre').evaluate(el=>getComputedStyle(el).visibility),'hidden');
  await highContrast.locator('#source').fill('link "console" as out\nemit Print { value = "high contrast" } to out');
  await highContrast.locator('#run').click();assert.equal(await highContrast.locator('#output').textContent(),'high contrast');
  await forced.close();
  // Coarse-pointer controls provide room for touch without hiding navigation.
  const touch=await browser.newContext({hasTouch:true,isMobile:true,viewport:{width:360,height:800},reducedMotion:'reduce'});
  const phone=await touch.newPage();await phone.goto(url('playground.html'),{waitUntil:'networkidle'});
  await phone.locator('#run:enabled').waitFor();
  for(const control of await phone.locator('.pg-feature, #run, #reset, .switch, .site-nav .nav-links a').all()){
    assert.ok((await control.boundingBox()).height>=44);
  }
  assert.equal(await phone.locator('#source').evaluate(el=>getComputedStyle(el).fontSize),'16px');
  assert.ok(await phone.locator('#expected-output').evaluate(el=>el.getBoundingClientRect().bottom<800));
  await phone.getByRole('link',{name:'View result ↓',exact:true}).click();
  assert.ok((await phone.locator('#output').boundingBox()).y >= await phone.locator('.site-nav').evaluate(el=>el.getBoundingClientRect().bottom));
  await phone.close();await touch.close();
  // Review captures from the final visible states, with controls/details in view.
  for(const [route,name] of [['index.html','home'],['learn.html','learn'],['learn/small-program.html','lesson'],['playground.html','playground']]){
    await page.goto(url(route),{waitUntil:'networkidle'});
    if(name==='playground') await page.locator('#run:enabled').waitFor();
    for(const image of await page.locator('img[loading="lazy"]').all()){
      await image.scrollIntoViewIfNeeded();await image.evaluate(el=>el.decode());
    }
    for(const width of [390,1440]){
      await page.setViewportSize({width,height:844});await page.evaluate(()=>scrollTo(0,0));
      await page.screenshot({path:`/var/tmp/code-224-${name}-${width}.png`,fullPage:true});
    }
  }
  assert.deepEqual(errors,[]);
  console.log(`${routes.length} pages at four widths, ${audit.length} expanded axe scans, ${cards} phone gallery cards, 200% text, keyboard/copy fallback, no-JS, forced colors, touch and captures passed. ${audit.reduce((n,x)=>n+x.incomplete.reduce((m,v)=>m+v.count,0),0)} automated review items retained in JSON; this is not a conformance certification.`);
} finally {writeFileSync('/var/tmp/code-224-accessibility.json',JSON.stringify(audit,null,2));await browser.close()}
