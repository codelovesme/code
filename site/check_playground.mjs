// First-use, advanced collection, draft and engine lifecycle checks in Chromium.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
const url = path => new URL(path, base).href;
async function waitFor(test, label) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await test()) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw Error(label);
}
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base.origin });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(url('playground.html'), { waitUntil: 'networkidle' });
  await waitFor(async () => !(await page.locator('#run').isDisabled()), 'engine not ready');
  assert.equal(await page.locator('#output').textContent(), 'hello, world');
  assert.equal(await page.locator('.ex-card').count(), 0);
  assert.equal(await page.locator('#all-examples').evaluate(el => el.open), false);
  const featured = await page.locator('#featured-data').evaluate(el => JSON.parse(el.textContent));
  const collection = await page.locator('#examples-data').evaluate(el => JSON.parse(el.textContent));
  assert.equal(featured.length, 4);
  for (const example of featured) {
    const button = page.locator(`[data-example="${example.name}"].pg-feature`);
    await button.focus(); await page.keyboard.press('Enter');
    assert.equal(await button.getAttribute('aria-pressed'), 'true');
    assert.equal(await page.locator('#source').inputValue(), example.code);
    assert.equal(await page.locator('#example-description').textContent(), example.description);
    assert.equal(await page.locator('#expected-output').textContent(), example.output);
    assert.equal(await page.locator('#output').textContent(), example.output);
    await page.locator('#source').fill(example.code + '\ninvalid_name');
    await page.locator('#reset').click();
    assert.equal(await page.locator('#source').inputValue(), example.code);
    assert.equal(await page.locator('#output').textContent(), example.output);
  }
  // Editing the divisor should also demonstrate the successful returned value.
  const division = featured.find(example => example.name === 'returned_failure');
  await page.locator('#source').fill(division.code.replace('right = 0 } to this', 'right = 4 } to this'));
  await page.locator('#run').click();
  assert.equal(await page.locator('#output').textContent(), '3');
  await page.locator('#reset').click();
  await page.locator('#copy-out').click();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), featured.at(-1).output);

  // A draft persists, including editor-inserted indentation, and leaves Tab via Escape.
  const draft = 'link "console" as out\nemit Print { value = "my draft" } to out';
  await page.locator('#source').fill(draft);
  await page.locator('#source').focus(); await page.keyboard.press('Control+Enter');
  assert.equal(await page.locator('#output').textContent(), 'my draft');
  await page.keyboard.press('Control+End'); await page.keyboard.press('Tab');
  const indentedDraft = await page.locator('#source').inputValue();
  assert.ok(indentedDraft.endsWith('    '));
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#run').evaluate(el => el === document.activeElement), true);
  await page.reload({ waitUntil: 'networkidle' });
  assert.equal(await page.locator('#source').inputValue(), indentedDraft);
  await page.locator('.switch-label').click();
  assert.equal(await page.locator('#live').isChecked(), true);
  await page.locator('#source').fill(draft.replace('my draft', 'live result'));
  await waitFor(async () => (await page.locator('#output').textContent()) === 'live result', 'auto-run failed');

  await page.locator('#source').fill('');
  await page.reload({ waitUntil: 'networkidle' });
  assert.equal(await page.locator('#source').inputValue(), '');
  assert.equal(await page.locator('#expected-panel').isVisible(), false);
  await page.locator('#reset').click();
  assert.equal(await page.locator('#output').textContent(), 'hello, world');

  // Explicit named sources and share links still win over that saved draft.
  await page.goto(url('playground.html?example=value_snapshot#playground'), { waitUntil: 'networkidle' });
  assert.equal(await page.locator('#output').textContent(), featured.find(example => example.name === 'value_snapshot').output);
  const shared = draft.replace('my draft', 'shared 🙂');
  await page.goto(url('index.html#src=' + encodeURIComponent(shared)), { waitUntil: 'networkidle' });
  assert.equal(await page.locator('#source').inputValue(), shared);
  assert.equal(await page.locator('#output').textContent(), 'shared 🙂');
  await page.locator('.pg-feature').first().click();
  assert.ok(!page.url().includes('#src='));

  // Visit every advanced page; there must be no missing or repeated fixtures.
  await page.goto(url('playground.html#examples'), { waitUntil: 'networkidle' });
  assert.equal(await page.locator('#all-examples').evaluate(el => el.open), true);
  const seen = [];
  while (true) {
    const names = await page.locator('.ex-card').evaluateAll(cards => cards.map(card => card.dataset.example));
    assert.ok(names.length > 0 && names.length <= 12);
    seen.push(...names);
    if (await page.locator('#next-examples').isDisabled()) break;
    await page.locator('#next-examples').click();
  }
  assert.equal(new Set(seen).size, seen.length);
  assert.deepEqual([...seen].sort(), collection.map(example => example.name).sort());
  await page.getByRole('button', { name: 'Printing', exact: true }).click();
  assert.equal(await page.locator('#previous-examples').isDisabled(), true);
  assert.equal(await page.getByRole('button', { name: 'Printing', exact: true }).getAttribute('aria-pressed'), 'true');
  await page.locator('#example-search').fill('value_snapshot');
  assert.equal(await page.locator('.ex-card').count(), 1);
  await page.locator('.ex-card').click();
  assert.equal(await page.locator('#source').evaluate(el => el === document.activeElement), true);
  assert.equal(await page.locator('#output').textContent(), 'original: Ada\nupdated: Lin');
  await page.locator('#example-search').fill('zzzz-no-example');
  assert.equal(await page.locator('.ex-card').count(), 0);
  assert.ok((await page.locator('#example-range').innerText()).includes('No matching'));
  assert.equal(await page.locator('#previous-examples').isDisabled(), true);
  assert.equal(await page.locator('#next-examples').isDisabled(), true);
  await page.locator('#example-search').fill('');

  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
    for (const button of await page.locator('.pg-feature').all()) assert.equal(await button.isVisible(), true);
    await page.locator('#source').fill(shared);
    await page.locator('#source').focus(); await page.keyboard.press('Control+Enter');
    assert.equal(await page.locator('#output').textContent(), 'shared 🙂');
    assert.equal(await page.evaluate(() => matchMedia('(prefers-reduced-motion: reduce)').matches), true);
    assert.equal(await page.locator('.blob').first().evaluate(el => getComputedStyle(el).animationName), 'none');
  }
  assert.deepEqual(errors, []);
  await context.close();

  // Exercise clicks/keyboard while WASM is pending, then run the latest selection.
  const loadingContext = await browser.newContext();
  const loading = await loadingContext.newPage();
  const lifecycleErrors = [];
  loading.on('pageerror', error => lifecycleErrors.push(error.message));
  let releaseDownload;
  const gate = new Promise(resolve => { releaseDownload = resolve; });
  await loading.route('**/*.wasm', async route => { await gate; await route.continue(); });
  await loading.goto(url('playground.html'), { waitUntil: 'domcontentloaded' });
  await loading.locator('.pg-feature').last().waitFor();
  assert.equal(await loading.locator('#run').isDisabled(), true);
  assert.equal(await loading.locator('#live').isDisabled(), true);
  await loading.locator('.pg-feature').last().click();
  await loading.locator('#source').focus(); await loading.keyboard.press('Control+Enter');
  assert.equal(await loading.locator('#status').innerText(), 'loading…');
  releaseDownload();
  await waitFor(async () => !(await loading.locator('#run').isDisabled()), 'delayed engine not ready');
  assert.equal(await loading.locator('#output').textContent(), 'Choose a non-zero divisor.');
  assert.deepEqual(lifecycleErrors, []);
  await loadingContext.close();

  for (const blockedPattern of ['**/*.wasm', '**/pkg/code_wasm.js']) {
    const broken = await browser.newPage();
    const brokenErrors = [];
    broken.on('pageerror', error => brokenErrors.push(error.message));
    await broken.route(blockedPattern, route => route.abort());
    await broken.goto(url('playground.html'), { waitUntil: 'networkidle' });
    await broken.locator('#status.err').waitFor();
    assert.equal(await broken.locator('#run').isDisabled(), true);
    assert.equal(await broken.locator('#live').isDisabled(), true);
    await broken.locator('.pg-feature').last().click();
    await broken.locator('#reset').click();
    await broken.locator('#source').focus(); await broken.keyboard.press('Control+Enter');
    assert.equal(await broken.locator('#status').innerText(), 'failed to load engine');
    assert.ok((await broken.locator('#output').textContent()).includes('reload the page'));
    assert.deepEqual(brokenErrors, []);
    await broken.close();
  }
  console.log(`Four featured outputs, all ${collection.length} fixtures via pagination, filter/search, draft/share/reset/copy, four widths and pending/failed engine flows passed`);
} finally { await browser.close(); }
