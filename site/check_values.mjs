// Values lesson: every linked program, edited result and exercise runs for real.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const plan = JSON.parse(readFileSync(new URL('lessons.json', import.meta.url), 'utf8'));
const lesson = plan.find(item => item.id === 'values');
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
const url = path => new URL(path, base).href;
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base.origin });
  const page = await context.newPage();
  const errors = [], missing = [], engineLoads = [];
  let staticPage = true;
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => { if (response.status() >= 400) missing.push(response.url()); });
  page.on('request', request => { if (staticPage && /\/pkg\/|\.wasm/.test(request.url())) engineLoads.push(request.url()); });
  await page.goto(url('learn.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: 'Values and control flow', exact: true }).click();
  await page.waitForURL('**/learn/values.html');
  assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Learn');
  assert.equal(await page.locator('#values-solution').evaluate(el => el.open), false);
  assert.equal(await page.locator('.lesson-neighbors a[rel="prev"]').getAttribute('href'), '../learn/first-program.html');
  assert.equal(await page.locator('.lesson-neighbors a[rel="next"]').getAttribute('href'), '../learn/messages.html');
  assert.equal(await page.locator('section[aria-labelledby="values-exercise"] > .lesson-result pre').textContent(), lesson.steps.at(-1).output);
  assert.deepEqual(engineLoads, []);
  for (const step of lesson.steps) {
    if (step.id === 'solution') {
      await page.locator('#values-solution summary').focus(); await page.keyboard.press('Enter');
    }
    const code = readFileSync(new URL(`examples/${step.example}.code`, import.meta.url), 'utf8');
    const panel = page.locator(`#values-${step.id}-code`).locator('..');
    assert.equal(await panel.locator('pre').textContent(), code.trim());
    await panel.getByRole('button', { name: 'Copy code', exact: true }).click();
    await panel.locator('[role="status"]').filter({ hasText: 'Code copied' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), code.trim());
    staticPage = false;
    await panel.getByRole('link', { name: `Run this example → ${step.title}`, exact: true }).click();
    await page.waitForURL(`**/playground.html?example=${step.example}#playground`);
    await page.locator('#run:enabled').waitFor();
    assert.equal((await page.locator('#source').inputValue()).trim(), code.trim());
    assert.equal(await page.locator('#output').textContent(), step.output, step.id);
    for (const edit of step.edits ?? []) {
      await page.locator('#source').fill(code.replace(edit.find, edit.replace));
      await page.locator('#run').click();
      assert.equal(await page.locator('#output').textContent(), edit.output);
    }
    await page.goBack({ waitUntil: 'networkidle' });
    staticPage = true;
  }
  staticPage = false;
  await page.getByRole('link', { name: 'Start the exercise →', exact: true }).click();
  await page.waitForURL('**/playground.html?example=learn_values_filter#playground');
  await page.locator('#run:enabled').waitFor();
  const solution = lesson.steps.at(-1);
  await page.locator('#source').fill(readFileSync(new URL(`examples/${solution.example}.code`, import.meta.url), 'utf8'));
  await page.locator('#run').click();
  assert.equal(await page.locator('#output').textContent(), solution.output);
  await page.goBack({ waitUntil: 'networkidle' }); staticPage = true;
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    if (!(await page.locator('#values-solution').evaluate(el => el.open))) await page.locator('#values-solution summary').click();
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
    for (const pre of await page.locator('pre').all()) {
      const box = await pre.boundingBox();
      if (box) assert.ok(box.x >= 0 && box.x + box.width <= width + 1);
    }
  }
  await page.getByRole('link', { name: 'Continue: handlers and messages →', exact: true }).click();
  await page.waitForURL('**/learn/messages.html');
  await page.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  await page.waitForURL('**/learn.html');
  assert.equal(await page.locator('#path-messages a').count(), 1);
  await page.getByRole('link', { name: 'Values and control flow', exact: true }).click();
  await page.locator('.lesson-neighbors a[rel="prev"]').click();
  await page.waitForURL('**/learn/first-program.html');
  await page.locator('.lesson-neighbors a[rel="next"]').click();
  await page.waitForURL('**/learn/values.html');
  await page.locator('.lesson-outline summary').focus(); await page.keyboard.press('Enter');
  assert.equal(await page.locator('[aria-current="step"]').innerText(), lesson.title);
  assert.deepEqual(errors, []); assert.deepEqual(missing, []); assert.deepEqual(engineLoads, []);
  const noJS = await browser.newContext({ javaScriptEnabled: false });
  const plain = await noJS.newPage();
  await plain.goto(url('learn/values.html'));
  await plain.locator('#values-solution summary').click();
  assert.ok((await plain.locator('#values-solution-code').textContent()).includes('updated[0].count = 4'));
  assert.equal(await plain.getByRole('button', { name: 'Copy code', exact: true }).count(), 0);
  await plain.getByRole('link', { name: '← Back to Learn', exact: true }).click();
  assert.ok(plain.url().endsWith('/learn.html'));
  await noJS.close();
  console.log('Values lesson: five canonical programs, two edits, independent-copy exercise/solution, copy/run links, four widths, keyboard/no-JS and ready neighbors passed');
} finally { await browser.close(); }
