// Final course lesson: real runs, downloads, clipboard and responsive navigation.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2]);
const url = path => new URL(path, base).href;
const lesson = JSON.parse(readFileSync(new URL('lessons.json', import.meta.url))).find(x => x.id === 'small-program');
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base.origin });
  const page = await context.newPage();
  const errors = [], missing = [], engines = [];
  let staticPage = true;
  page.on('pageerror', e => errors.push(e.message));
  page.on('response', r => { if (r.status() >= 400) missing.push(r.url()); });
  page.on('request', r => { if (staticPage && /\/pkg\/|\.wasm/.test(r.url())) engines.push(r.url()); });
  await page.goto(url('learn.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: lesson.title, exact: true }).click();
  await page.waitForURL('**/learn/small-program.html');
  assert.equal(await page.locator('.lesson-neighbors a[rel="next"]').count(), 0);
  assert.equal(await page.locator('.lesson-neighbors a[rel="prev"]').getAttribute('href'), '../learn/messages.html');
  assert.equal(await page.locator('#cart-invalid').evaluate(el => el.open), false);
  for (const step of lesson.steps) {
    if (step.id === 'invalid') { await page.locator('#cart-invalid summary').focus(); await page.keyboard.press('Enter'); }
    const code = readFileSync(new URL(`examples/${step.example}.code`, import.meta.url), 'utf8');
    const panel = page.locator(`#small-program-${step.id}-code`).locator('..');
    assert.equal(await panel.locator('pre').textContent(), code.trim());
    await panel.getByRole('button', { name: 'Copy code', exact: true }).click();
    await panel.locator('[role="status"]').filter({ hasText: 'Code copied' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), code.trim());
    staticPage = false;
    await panel.getByRole('link', { name: `Run example: ${step.title}`, exact: true }).click();
    await page.locator('#run:enabled').waitFor();
    assert.equal(await page.locator('#output').textContent(), step.output);
    assert.equal(await page.locator('#status').evaluate(el => el.classList.contains('err')), false);
    for (const edit of step.edits ?? []) {
      await page.locator('#source').fill(code.replace(edit.find, edit.replace));
      await page.locator('#run').click();
      assert.equal(await page.locator('#output').textContent(), edit.output);
    }
    await page.goBack({ waitUntil: 'networkidle' }); staticPage = true;
  }
  const download = await page.request.get(url('lesson-examples/cart/receipt.code'));
  assert.equal(download.status(), 200);
  assert.equal(await download.text(), readFileSync(new URL('examples/learn_cart_final.code', import.meta.url), 'utf8'));
  await page.locator('#cart-module summary').focus(); await page.keyboard.press('Enter');
  for (const [id, name, label] of [['cart-module-code','cart.code','Copy cart.code'], ['cart-caller-code','main.code','Copy main.code'], ['cart-run','run.sh','Copy local commands']]) {
    const response = await page.request.get(url(`lesson-examples/cart/${name}`));
    assert.equal(response.status(), 200);
    const content = (await response.text()).trim();
    const panel = page.locator(`#${id}`).locator('..');
    assert.equal(await page.locator(`#${id}`).textContent(), content);
    await panel.getByRole('button', { name: label, exact: true }).click();
    await panel.locator('[role="status"]').filter({ hasText: name.endsWith('.code') ? 'named file' : 'terminal' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), content);
    assert.equal(await panel.locator('a[href*="playground"]').count(), 0);
  }
  for (const width of [360,390,768,1440]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
  }
  await page.screenshot({ path: '/var/tmp/code-222-desktop.png', fullPage: true });
  await page.setViewportSize({ width:390, height:844 });
  await page.screenshot({ path: '/var/tmp/code-222-mobile.png', fullPage:true });
  for (const [label, route] of [['Explore modules and reuse →','modules.html'],['Language reference →','docs.html#language-reference'],['Choose how to run it →','execution.html']]) {
    await page.getByRole('link', { name:label, exact:true }).click();
    await page.waitForURL(url(route)); await page.goBack({ waitUntil:'networkidle' });
  }
  assert.deepEqual(errors, []); assert.deepEqual(missing, []); assert.deepEqual(engines, []);
  const noJS = await browser.newContext({ javaScriptEnabled:false });
  const plain = await noJS.newPage(); await plain.goto(url('learn/small-program.html'));
  await plain.locator('#cart-module summary').click(); await plain.locator('#cart-invalid summary').click();
  assert.ok((await plain.locator('#cart-module-code').textContent()).includes('LineTotal'));
  assert.equal(await plain.getByRole('button', { name:'Copy cart.code', exact:true }).isVisible(), false);
  await noJS.close();
  console.log('Cart: four actual browser programs/three edits, exact downloads/copy, local boundaries, keyboard/no-JS, four widths and finish links passed');
} finally { await browser.close(); }
