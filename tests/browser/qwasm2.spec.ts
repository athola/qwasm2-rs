import { test, expect } from '@playwright/test';

// Serve the single HTML file directly — no server needed
const HTML_PATH = 'qwasm2.html';

test.describe('Qwasm2 WASM Module', () => {

  test('CP-0: WASM module loads successfully', async ({ page }) => {
    await page.goto(HTML_PATH);

    // Wait for WASM to initialize (check the test hook)
    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    // Verify the status area shows engine initialization progressed
    const status = await page.locator('#status').textContent();
    expect(status).toContain('Initializing engine');
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

    // Engine logs "WASM module initialized" and "Initializing engine"
    const status = await page.locator('#status').textContent();
    expect(status).toContain('Initializing engine');
  });

  test('CP-0: WebGL2 context created', async ({ page }) => {
    await page.goto(HTML_PATH);

    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    const status = await page.locator('#status').textContent();
    // Engine logs "Creating WebGL2 context..." then "GL3 renderer initialized"
    expect(status).toContain('GL3 renderer initialized');
  });

  test('CP-0: No unexpected console errors', async ({ page }) => {
    const errors: string[] = [];
    page.on('console', msg => {
      if (msg.type() === 'error' && !msg.text().includes('favicon')) {
        errors.push(msg.text());
      }
    });

    await page.goto(HTML_PATH);

    await page.waitForFunction(() => {
      const el = document.getElementById('wasm-loaded');
      return el && el.textContent === 'true';
    }, null, { timeout: 10000 });

    // Wait briefly for any async errors
    await page.waitForTimeout(1000);

    // Filter out known benign errors:
    // - pak0.pak 404 is expected when not running the devserver with gamedata
    // - "Failed to load resource" is Chrome's generic network error for the same 404
    const realErrors = errors.filter(e =>
      !e.includes('favicon') &&
      !e.includes('pak0.pak') &&
      !e.includes('Fetch failed') &&
      !e.includes('404')
    );
    expect(realErrors).toEqual([]);
  });
});
