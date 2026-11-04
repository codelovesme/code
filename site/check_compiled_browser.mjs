// Verify the bundle produced by check_execution.py in an actual browser.
import assert from 'node:assert/strict';
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright');
const browser = await chromium.launch({ headless: true, channel: 'chromium' });
try {
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(process.argv[2] ?? 'http://127.0.0.1:8956/');
  await page.getByText('Compiled program finished.', { exact: true }).waitFor();
  assert.equal((await page.locator('#output').innerText()).trim(), 'hello, world');
  assert.deepEqual(errors, []);
  await page.route('**/hello.wasm', route => route.abort());
  await page.reload();
  await page.waitForFunction(() => document.querySelector('#status').textContent.startsWith('Could not run the compiled program:'));
  assert.equal(await page.locator('#output').innerText(), '');
  assert.deepEqual(errors, []);
  console.log('Published v2.23.0 compiled WASM: real Chromium output and missing-artifact handling passed');
} finally {
  await browser.close();
}
