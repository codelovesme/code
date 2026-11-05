// The Learn path works at /code/, including direct nested routes and no-JS copy.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const source = readFileSync(new URL('examples/hello_home.code', import.meta.url), 'utf8').trim();
const plan = JSON.parse(readFileSync(new URL('lessons.json', import.meta.url), 'utf8'));
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
const url = path => new URL(path, base).href;
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base.origin });
  const page = await context.newPage();
  const errors = [], missing = [], engineRequests = [];
  let staticPage = true;
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => { if (response.status() >= 400) missing.push(response.url()); });
  page.on('request', request => { if (staticPage && /\/pkg\/|\.wasm/.test(request.url())) engineRequests.push(request.url()); });
  await page.goto(url('index.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: 'Start learning', exact: true }).click();
  await page.waitForURL('**/learn.html');
  assert.equal(await page.locator('#learning-path .learning-path li').count(), 4);
  assert.equal(await page.locator('#learning-path a').count(), plan.slice(1).filter(lesson => lesson.ready).length);
  for (const lesson of plan.filter(item => !item.ready)) {
    assert.equal(await page.locator(`a[href$="${lesson.route}"]`).count(), 0);
  }
  await page.getByRole('link', { name: 'Quick start →', exact: true }).click();
  await page.waitForURL('**/learn/quick-start.html');
  assert.equal(await page.locator('#greeting-code').textContent(), source);
  assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Learn');
  assert.equal(await page.locator('.lesson-neighbors a[rel="next"]').getAttribute('href'), '../learn/first-program.html');
  await page.getByRole('button', { name: 'Copy code', exact: true }).click();
  await page.locator('#lesson-copy-status').filter({ hasText: 'Code copied' }).waitFor();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), source);
  await page.evaluate(() => { navigator.clipboard.writeText = async () => { throw Error('test clipboard refusal'); }; });
  await page.getByRole('button', { name: 'Copy code', exact: true }).click();
  await page.locator('#lesson-copy-status').filter({ hasText: 'Code selected' }).waitFor();
  assert.equal(await page.evaluate(() => getSelection().toString()), source);
  assert.deepEqual(engineRequests, []);
  staticPage = false;
  await page.getByRole('link', { name: 'Open and run →', exact: true }).click();
  await page.waitForURL('**/playground.html?example=hello_home#playground');
  await page.locator('#run:enabled').waitFor();
  assert.equal(await page.locator('#output').innerText(), 'hello, world');
  await page.locator('#source').fill(source.replace('name = "world"', 'name = "Ada"'));
  await page.locator('#run').click();
  assert.equal(await page.locator('#output').innerText(), 'hello, Ada');
  await page.goBack({ waitUntil: 'networkidle' });
  await page.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  await page.waitForURL('**/learn.html');
  staticPage = true;
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    for (const route of ['learn.html', 'learn/quick-start.html']) {
      await page.goto(url(route), { waitUntil: 'networkidle' });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${route}: ${width}px overflow`);
      assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Learn');
      if (route.includes('quick-start')) {
        const box = await page.locator('#greeting-code').boundingBox();
        assert.ok(box.x >= 0 && box.x + box.width <= width);
        await page.locator('.lesson-outline summary').focus(); await page.keyboard.press('Enter');
        assert.equal(await page.locator('[aria-current="step"]').innerText(), 'Try a change');
        assert.equal(await page.locator('.lesson-outline a').count(), plan.filter(lesson => lesson.ready).length);
        await page.keyboard.press('Space');
        assert.equal(await page.locator('.lesson-outline').evaluate(el => el.open), false);
        await page.locator('.skip-link').focus(); await page.keyboard.press('Enter');
        assert.ok(page.url().endsWith('/learn/quick-start.html#main'));
      }
    }
  }
  await page.locator('.site-nav').getByRole('link', { name: 'Docs', exact: true }).click();
  await page.waitForURL('**/docs.html');
  await page.locator('.site-footer').getByRole('link', { name: 'Learn', exact: true }).click();
  await page.waitForURL('**/learn.html');
  assert.deepEqual(engineRequests, []);
  assert.deepEqual(errors, []);
  assert.deepEqual(missing, []);
  // Static lessons remain readable and navigable without JavaScript.
  const noJS = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noJS.newPage();
  await plain.goto(url('learn/quick-start.html'));
  assert.equal(await plain.locator('#greeting-code').textContent(), source);
  assert.equal(await plain.getByRole('button', { name: 'Copy code', exact: true }).isVisible(), false);
  assert.equal(await plain.getByRole('link', { name: 'Open and run →', exact: true }).count(), 1);
  await plain.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  assert.ok(plain.url().endsWith('/learn.html'));
  await noJS.close();
  console.log('Learn hub, ready-only links, direct nested lesson, copy/fallback, edited output, back/reference navigation and 4 widths passed; no runtime loaded on lessons');
} finally { await browser.close(); }
