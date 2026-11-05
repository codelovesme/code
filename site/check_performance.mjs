// Focused evidence-page checks. This page must work without a runtime engine.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const base = new URL(process.argv[2] ?? 'http://127.0.0.1:8955/code/');
const report = JSON.parse(readFileSync(new URL('../benchmarks/website-evidence/results/2026-11-04-v2.23.0/report.json', import.meta.url)));
const summaries = new Map(report.summary.map(row => [[row.group, row.workload, row.mode].join('|'), row]));
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
try {
  const context = await browser.newContext({ reducedMotion: 'reduce' });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const engineRequests = [];
  page.on('request', request => { if (/\.wasm|\/pkg\//.test(request.url())) engineRequests.push(request.url()); });
  await page.goto(new URL('index.html', base).href, { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: 'Native speed, cached runs and memory' }).click();
  await page.waitForURL('**/compare.html#performance');
  assert.equal(await page.locator('.site-nav [aria-current="page"]').innerText(), 'Performance');
  assert.equal(await page.locator('details[open]').count(), 0);
  for (const number of await page.locator('[data-evidence]').all()) {
    const parts = (await number.getAttribute('data-evidence')).split('|');
    const metric = parts.pop();
    const row = summaries.get(parts.join('|'));
    assert.ok(row, parts.join('|'));
    const value = metric === 'time' ? row.median_seconds * 1000 : row.median_peak_rss_kib / 1024;
    assert.equal(await number.textContent(), value.toFixed(3));
  }
  const oldAnchors = ['at-a-glance', 'matrix-title', 'performance', 'compiled-runtime-chart-title', 'source-runtime-chart-title', 'compile-chart-title', 'memory', 'memory-chart-title', 'types-title', 'structure-title', 'errors-title', 'concurrency-title', 'fit', 'sources-title'];
  for (const id of oldAnchors) assert.equal(await page.locator(`[id="${id}"]`).count(), 1, id);
  for (const width of [360, 390, 768, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    for (const summary of await page.locator('summary').all()) {
      await summary.focus();
      await page.keyboard.press('Enter');
      assert.equal(await summary.evaluate(el => el.parentElement.open), true);
    }
    assert.ok((await page.locator('body').innerText()).includes('slower than Rust in all five'));
    assert.ok((await page.locator('#memory').innerText()).includes('Rust Rc COW'));
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, `${width}px overflow`);
    // No clipped cells hidden behind body overflow-x; scroll regions remain usable.
    for (const table of await page.locator('table').all()) {
      const box = await table.boundingBox();
      assert.ok(box.x >= 0 && box.x + box.width <= width + 1, `table at ${width}px`);
    }
    for (const summary of await page.locator('summary').all()) {
      await summary.focus(); await page.keyboard.press('Space');
      assert.equal(await summary.evaluate(el => el.parentElement.open), false);
    }
  }
  for (const anchor of ['performance', 'memory', 'fit', 'compile-chart-title']) {
    await page.goto(new URL(`compare.html#${anchor}`, base).href, { waitUntil: 'networkidle' });
    assert.equal(await page.locator(`#${anchor}`).isVisible(), true);
  }
  const sourceLinks = await page.locator('a[href*="github.com/codelovesme/code/blob/"]').evaluateAll(links => links.map(link => link.href));
  assert.ok(sourceLinks.length >= 6);
  assert.ok(sourceLinks.every(link => link.includes('/65403bd/')));
  assert.deepEqual(engineRequests, []);
  assert.deepEqual(errors, []);
  console.log('Performance values, pinned sources, 14 legacy targets, native details/keyboard and 4 viewport widths passed; no engine requested');
} finally { await browser.close(); }
