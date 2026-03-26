import { test, expect } from '@playwright/test';

test('debug: capture all console output', async ({ page }) => {
  const logs: string[] = [];
  page.on('console', msg => {
    logs.push(`[${msg.type()}] ${msg.text()}`);
  });
  page.on('pageerror', err => {
    logs.push(`[PAGE_ERROR] ${err.message}`);
  });

  await page.goto('qwasm2.html');
  await page.waitForTimeout(5000);

  const html = await page.content();
  const bodyText = await page.locator('body').textContent();

  console.log('=== CONSOLE LOGS ===');
  for (const log of logs) {
    console.log(log);
  }
  console.log('=== PAGE TEXT ===');
  console.log(bodyText?.substring(0, 500));
  console.log('=== TEST HOOKS ===');
  const wasmLoaded = await page.locator('#wasm-loaded').textContent().catch(() => 'NOT FOUND');
  const testResult = await page.locator('#self-test-result').textContent().catch(() => 'NOT FOUND');
  console.log('wasm-loaded:', wasmLoaded);
  console.log('self-test-result:', testResult);

  // This test always passes — it's just for debugging
  expect(true).toBe(true);
});
