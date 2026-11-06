// Lesson 1's public browser flow and exact downloadable Linux instructions.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const source = readFileSync(new URL('examples/hello_home.code', import.meta.url), 'utf8').trim();
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
const url = path => new URL(path, base).href;
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base.origin });
  const page = await context.newPage();
  const errors = [], missing = [], engines = [];
  let staticPage = true;
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => { if (response.status() >= 400) missing.push(response.url()); });
  page.on('request', request => { if (staticPage && /\/pkg\/|\.wasm/.test(request.url())) engines.push(request.url()); });
  await page.goto(url('index.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: 'Write your first program →', exact: true }).click();
  await page.waitForURL('**/learn/first-program.html');
  assert.equal(await page.locator('#first-program-code').textContent(), source);
  assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Learn');
  assert.equal(await page.locator('#linux-setup').evaluate(el => el.open), false);
  assert.equal(await page.locator('.lesson-neighbors a[rel="prev"]').getAttribute('href'), '../learn/quick-start.html');
  assert.equal(await page.locator('.lesson-neighbors a[rel="next"]').getAttribute('href'), '../learn/values.html');
  await page.getByRole('button', { name: 'Copy code', exact: true }).click();
  await page.locator('#lesson-copy-status').filter({ hasText: 'Code copied' }).waitFor();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), source);
  assert.deepEqual(engines, []);
  staticPage = false;
  await page.getByRole('link', { name: 'Run this program →', exact: true }).click();
  await page.waitForURL('**/playground.html?example=hello_home#playground');
  await page.locator('#run:enabled').waitFor();
  assert.equal(await page.locator('#output').textContent(), 'hello, world');
  await page.locator('#source').fill(source.replace('name = "world"', 'name = "Ada"'));
  await page.locator('#run').click();
  assert.equal(await page.locator('#output').textContent(), 'hello, Ada');
  await page.goBack({ waitUntil: 'networkidle' });
  staticPage = true;
  await page.locator('#linux-setup summary').focus(); await page.keyboard.press('Enter');
  assert.equal(await page.locator('#linux-setup').evaluate(el => el.open), true);
  const scripts = [];
  for (const [id, file, name] of [['cli-install', 'install', 'Copy download commands'], ['cli-project', 'project', 'Copy project commands'], ['cli-run', 'run', 'Copy run commands']]) {
    const response = await page.request.get(url(`lesson-examples/first-program/${file}.sh`));
    assert.equal(response.status(), 200);
    const text = await response.text(); scripts.push(text);
    assert.equal(await page.locator(`#${id}`).textContent(), text.trim());
    await page.getByRole('button', { name, exact: true }).click();
    await page.locator(`#${id}`).locator('..').locator('[role="status"]').filter({ hasText: 'Commands copied' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), text.trim());
    assert.ok((await page.locator(`#${id}`).locator('..').locator('[role="status"]').textContent()).includes('Commands copied'));
  }
  const all = await page.request.get(url('lesson-examples/first-program/start.sh'));
  assert.equal(await all.text(), '#!/bin/sh\nset -eu\n' + scripts.join('\n'));
  const downloaded = await page.request.get(url('lesson-examples/first-program/hello.code'));
  assert.equal((await downloaded.text()).trim(), source);
  assert.ok((await page.locator('#linux-setup').innerText()).includes('code-driver'));
  assert.ok((await page.locator('#linux-setup').innerText()).includes('Visual Studio Code'));
  assert.equal(await page.locator('a[href*="v2.23.2/README.md#running-it"]').count(), 1);
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
    for (const pre of await page.locator('pre').all()) {
      const box = await pre.boundingBox();
      if (box) assert.ok(box.x >= 0 && box.x + box.width <= width + 1);
    }
  }
  await page.getByRole('link', { name: 'Continue: values and control flow →', exact: true }).click();
  await page.waitForURL('**/learn/values.html');
  await page.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  await page.waitForURL('**/learn.html');
  assert.equal(await page.locator('#path-values a').count(), 1);
  await page.getByRole('link', { name: 'Your first program', exact: true }).click();
  await page.waitForURL('**/learn/first-program.html');
  await page.locator('.lesson-neighbors a[rel="prev"]').click();
  await page.waitForURL('**/learn/quick-start.html');
  await page.locator('.lesson-neighbors a[rel="next"]').click();
  await page.waitForURL('**/learn/first-program.html');
  await page.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  await page.waitForURL('**/learn.html');
  assert.deepEqual(engines, []); assert.deepEqual(errors, []); assert.deepEqual(missing, []);
  const noJS = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noJS.newPage();
  await plain.goto(url('learn/first-program.html'));
  assert.equal(await plain.locator('#first-program-code').textContent(), source);
  assert.equal(await plain.getByRole('button', { name: 'Copy code', exact: true }).isVisible(), false);
  await plain.locator('#linux-setup summary').click();
  assert.equal(await plain.locator('#cli-run').isVisible(), true);
  assert.equal(await plain.getByRole('button', { name: 'Copy run commands', exact: true }).isVisible(), false);
  await noJS.close();
  console.log('Lesson 1: Home/hub/direct route, browser edit/output, four exact copied/downloaded instruction assets, four widths, ready neighbors and no-JS content passed');
} finally { await browser.close(); }
