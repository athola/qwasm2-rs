import { test, expect } from '@playwright/test';

// Serve the single HTML file directly — no server needed
const HTML_PATH = 'qwasm2.html';

test.describe('Qwasm2 WASM Module', () => {

  test('CP-0: WASM module loads successfully', async ({ page }) => {
    // file:// URLs can't load WASM modules. Use a local server instead.
    // The test config should serve dist/ directory.
    await page.goto(HTML_PATH);

    // Wait for WASM to initialize (check the test hook)
    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    // Verify the status area shows successful load
    const status = await page.locator('#status').textContent();
    expect(status).toContain('WASM module loaded');
  });

  test('CP-0: Self-test passes in browser', async ({ page }) => {
    await page.goto(HTML_PATH);

    // Wait for self-test result
    await page.waitForFunction(() => {
      const el = document.getElementById('self-test-result');
      return el && el.textContent && el.textContent.length > 0;
    }, null, { timeout: 10000 });

    const result = await page.locator('#self-test-result').textContent();
    expect(result).toBe('PASS');
  });

  test('CP-0: Engine version is reported', async ({ page }) => {
    await page.goto(HTML_PATH);

    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    const status = await page.locator('#status').textContent();
    expect(status).toContain('qwasm2-rs');
  });

  test('CP-0: WebGL2 check runs without error', async ({ page }) => {
    await page.goto(HTML_PATH);

    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    const status = await page.locator('#status').textContent();
    // Should contain WebGL2 status (supported or not, but no error)
    expect(status).toContain('WebGL2:');
  });

  test('CP-0: No console errors', async ({ page }) => {
    const errors: string[] = [];
    page.on('console', msg => {
      if (msg.type() === 'error' && !msg.text().includes('favicon')) {
        errors.push(msg.text());
      }
    });

    await page.goto(HTML_PATH);

    await page.waitForFunction(() => {
      const el = document.getElementById('self-test-result');
      return el && el.textContent === 'PASS';
    }, null, { timeout: 10000 });

    // Filter out known benign errors
    const realErrors = errors.filter(e => !e.includes('favicon'));
    expect(realErrors).toEqual([]);
  });
});
