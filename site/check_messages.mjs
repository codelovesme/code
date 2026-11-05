// Lesson 3: actual browser requests/results and explicitly local source files.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const plan = JSON.parse(readFileSync(new URL('lessons.json', import.meta.url), 'utf8'));
const lesson = plan.find(item => item.id === 'messages');
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
  await page.goto(url('learn.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: lesson.title, exact: true }).click();
  await page.waitForURL('**/learn/messages.html');
  assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Learn');
  assert.equal(await page.locator('#messages-solution').evaluate(el => el.open), false);
  assert.equal(await page.locator('.lesson-neighbors a[rel="prev"]').getAttribute('href'), '../learn/values.html');
  assert.equal(await page.locator('.lesson-neighbors a[rel="next"]').getAttribute('href'), '../learn/small-program.html');
  for (const step of lesson.steps) {
    if (step.id === 'solution') { await page.locator('#messages-solution summary').focus(); await page.keyboard.press('Enter'); }
    const code = readFileSync(new URL(`examples/${step.example}.code`, import.meta.url), 'utf8');
    const panel = page.locator(`#messages-${step.id}-code`).locator('..');
    assert.equal(await panel.locator('pre').textContent(), code.trim());
    await panel.getByRole('button', { name: 'Copy code', exact: true }).click();
    await panel.locator('[role="status"]').filter({ hasText: 'Code copied' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), code.trim());
    staticPage = false;
    await panel.getByRole('link', { name: `Run this example → ${step.title}`, exact: true }).click();
    await page.waitForURL(`**/playground.html?example=${step.example}#playground`);
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
  staticPage = false;
  await page.getByRole('link', { name: 'Start the exercise →', exact: true }).click();
  await page.waitForURL('**/playground.html?example=learn_messages_failure#playground');
  await page.locator('#run:enabled').waitFor();
  await page.locator('#source').fill(readFileSync(new URL('examples/learn_messages_solution.code', import.meta.url), 'utf8'));
  await page.locator('#run').click();
  assert.equal(await page.locator('#output').textContent(), lesson.steps.at(-1).output);
  await page.goBack({ waitUntil: 'networkidle' }); staticPage = true;
  await page.locator('#local-reuse summary').focus(); await page.keyboard.press('Enter');
  for (const [id, filename, title] of [['greeter-code','greeter.code','Copy greeter.code'], ['caller-code','main.code','Copy main.code'], ['messages-run','run.sh','Copy local commands']]) {
    const response = await page.request.get(url(`lesson-examples/messages/${filename}`));
    assert.equal(response.status(), 200);
    const content = await response.text();
    const panel = page.locator(`#${id}`).locator('..');
    assert.equal(await page.locator(`#${id}`).textContent(), content.trim());
    await panel.getByRole('button', { name: title, exact: true }).click();
    await panel.locator('[role="status"]').filter({ hasText: filename.endsWith('.code') ? 'named file' : 'terminal' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), content.trim());
    assert.equal(await panel.locator('a[href*="playground"]').count(), 0);
  }
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    if (!(await page.locator('#messages-solution').evaluate(el => el.open))) await page.locator('#messages-solution summary').click();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
    for (const pre of await page.locator('pre').all()) {
      const box = await pre.boundingBox(); if (box) assert.ok(box.x >= 0 && box.x + box.width <= width + 1);
    }
  }
  await page.locator('.lesson-neighbors a[rel="next"]').click();
  await page.waitForURL('**/learn/small-program.html');
  await page.goto(url('learn.html'));
  await page.getByRole('link', { name: lesson.title, exact: true }).click();
  await page.locator('.lesson-neighbors a[rel="prev"]').click();
  await page.waitForURL('**/learn/values.html');
  await page.locator('.lesson-neighbors a[rel="next"]').click();
  await page.waitForURL('**/learn/messages.html');
  assert.deepEqual(errors, []); assert.deepEqual(missing, []); assert.deepEqual(engines, []);
  const noJS = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noJS.newPage(); await plain.goto(url('learn/messages.html'));
  await plain.locator('#messages-solution summary').click(); await plain.locator('#local-reuse summary').click();
  assert.ok((await plain.locator('#greeter-code').textContent()).includes('greeting = "hello"'));
  assert.equal(await plain.getByRole('button', { name: 'Copy greeter.code', exact: true }).isVisible(), false);
  await noJS.close();
  console.log('Messages: three actual browser examples, edits/handled failure/exercise, exact local-file downloads/copy feedback, four widths, keyboard/no-JS and ready neighbors passed');
} finally { await browser.close(); }
