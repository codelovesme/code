// Real-browser route/legacy-share regression checks. See site/README.md.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
const context = await browser.newContext({ reducedMotion: 'reduce' });
const page = await context.newPage();
const errors = [];
page.on('pageerror', error => errors.push(error.message));
const url = path => new URL(path, base).href;
let checks = 0;
async function waitFor(test, label) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await test()) return;
    await page.waitForTimeout(100);
  }
  throw Error(label);
}
try {
  for (const file of ['index.html', 'learn.html', 'playground.html', 'docs.html', 'compare.html', 'modules.html']) {
    await page.goto(url(file), { waitUntil: 'networkidle' });
    assert.equal(await page.locator('.site-nav a').count(), 5, file);
    for (const label of ['Learn', 'Playground', 'Docs', 'GitHub']) {
      assert.equal(await page.locator('.site-nav').getByRole('link', { name: label, exact: label !== 'GitHub' }).count(), 1);
    }
    await page.keyboard.press('Tab');
    assert.equal(await page.locator('.skip-link').evaluate(el => el === document.activeElement), true, file);
    await page.keyboard.press('Enter');
    await page.waitForURL('**#main');
    for (const width of [360, 390, 768, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${file} at ${width}px overflows`);
      for (const link of await page.locator('.site-nav .nav-links a').all()) assert.equal(await link.isVisible(), true);
    }
    if (file === 'index.html') assert.equal(await page.locator('.ex-card').count(), 0);
    if (file === 'playground.html') assert.ok(await page.locator('.ex-card').count() > 100);
    checks++;
  }

  for (const [hash, destination] of Object.entries({
    '#playground': 'playground.html#playground', '#examples': 'playground.html#examples',
    '#packages': 'docs.html#packages', '#features': 'docs.html#features',
  })) {
    for (const entry of ['', 'index.html']) {
      await page.goto(url(entry + hash), { waitUntil: 'networkidle' });
      await page.waitForURL(url(destination));
      checks++;
    }
  }

  const shared = 'link "console" as out\nemit Print { value = "shared 🙂" } to out';
  await page.goto(url('index.html#src=' + encodeURIComponent(shared)), { waitUntil: 'networkidle' });
  await waitFor(async () => (await page.locator('#output').innerText()).includes('shared 🙂'), 'legacy shared program did not run');
  assert.equal(await page.locator('#source').inputValue(), shared);
  checks++;

  const saved = 'link "console" as out\nemit Print { value = "saved draft" } to out';
  await page.locator('#source').fill(saved);
  await page.locator('#run').click();
  await page.goto(url('index.html'), { waitUntil: 'networkidle' });
  await page.goto(url('index.html#playground'), { waitUntil: 'networkidle' });
  assert.equal(await page.locator('#source').inputValue(), saved);
  await waitFor(async () => (await page.locator('#output').innerText()).includes('saved draft'), 'draft did not survive the route split');
  checks++;

  await page.goto(url('learn.html'), { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: 'Try this example' }).click();
  await page.waitForURL('**/playground.html?example=hello_home#playground');
  await waitFor(async () => (await page.locator('#output').innerText()).includes('hello, world'), 'Learn example did not run');
  await page.locator('#source').focus();
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#run').evaluate(el => el === document.activeElement), true);
  await page.locator('#live').focus();
  await page.keyboard.press('Space');
  assert.equal(await page.locator('#live').isChecked(), true);
  await page.locator('#source').fill('link "console" as out\nemit Print { value = "auto-run" } to out');
  await waitFor(async () => (await page.locator('#output').innerText()).includes('auto-run'), 'auto-run failed');
  await page.locator('#reset').click();
  assert.ok((await page.locator('#source').inputValue()).includes('name = "world"'));
  await page.goBack({ waitUntil: 'networkidle' });
  assert.equal(new URL(page.url()).pathname, new URL('learn.html', base).pathname);
  await page.locator('.site-nav').getByRole('link', { name: 'Docs', exact: true }).click();
  await page.locator('a[href="compare.html"]').first().click();
  assert.ok(page.url().endsWith('/compare.html'));
  await page.locator('.site-nav').getByRole('link', { name: 'Docs', exact: true }).click();
  assert.ok(page.url().endsWith('/docs.html'));
  checks++;

  await page.goto(url('playground.html#examples'), { waitUntil: 'networkidle' });
  const printing = page.getByRole('button', { name: 'Printing', exact: true });
  await printing.focus(); await page.keyboard.press('Enter');
  assert.equal(await page.locator('.chip.active').innerText(), 'Printing');
  await page.locator('.ex-card:visible').first().click();
  await waitFor(async () => !(await page.locator('#run').isDisabled()), 'engine not ready');
  assert.ok((await page.locator('#output').innerText()).length > 0);
  checks++;

  const blocked = await context.newPage();
  await blocked.route('**/*.wasm', route => route.abort());
  await blocked.goto(url('playground.html'), { waitUntil: 'networkidle' });
  await blocked.locator('#status.err').waitFor();
  assert.equal(await blocked.locator('#status').innerText(), 'failed to load wasm');
  assert.equal(await blocked.locator('#run').isDisabled(), true);
  assert.ok((await blocked.locator('#output').innerText()).startsWith('error:'));
  await blocked.close();
  checks++;

  assert.deepEqual(errors, []);
  console.log(`${checks} browser journeys passed; 6 pages checked at 4 viewport widths`);
} finally {
  await browser.close();
}
