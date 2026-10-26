import { test, expect, Page } from '@playwright/test';

// dist/qwasm2.html is served by the webServer in playwright.config.ts. That
// server has no /gamedata, so start_game always ends with the pak fetch 404.
const HTML_PATH = 'qwasm2.html';
const PAK_PATH = '/gamedata/baseq2/pak0-web.pak';

// Wait for WASM init, then for start_game to reach a terminal state
// (failure line or running engine) so every log line it writes is present.
async function loadUntilSettled(page: Page) {
  await page.goto(HTML_PATH);
  await expect(page.locator('#wasm-loaded')).toHaveText('true', { timeout: 10000 });
  await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 10000 });
}

test.describe('Qwasm2 WASM Module', () => {

  test('CP-0: WASM module loads and engine initializes', async ({ page }) => {
    await loadUntilSettled(page);
    await expect(page.locator('#status')).toContainText('Initializing engine');
  });

  test('CP-0: Self-test passes in browser', async ({ page }) => {
    await loadUntilSettled(page);
    await expect(page.locator('#self-test-result')).toHaveText('PASS');
  });

  test('CP-0: Engine version is reported', async ({ page }) => {
    await loadUntilSettled(page);
    await expect(page.locator('#engine-version')).toHaveText(/^qwasm2-rs \d+\.\d+\.\d+$/);
  });

  test('CP-0: WebGL2 context created', async ({ page }) => {
    await loadUntilSettled(page);
    await expect(page.locator('#status')).toContainText('GL3 renderer initialized');
  });

  test('CP-0: Startup error is shown as text', async ({ page }) => {
    await loadUntilSettled(page);
    const failure = page.locator('#status .fail');
    await expect(failure).toHaveText(/^ERROR: Fetch failed: 404/);
    // Built with textContent: no markup from the error message becomes elements.
    await expect(failure.locator('*')).toHaveCount(0);
  });

  test('CP-0: No unexpected console errors', async ({ page }) => {
    const errors: { text: string; url: string }[] = [];
    page.on('console', msg => {
      if (msg.type() === 'error') {
        errors.push({ text: msg.text(), url: msg.location().url });
      }
    });

    await loadUntilSettled(page);

    // Expected without gamedata: the pak 404 (Chrome reports the URL only in
    // the message location), the page's own log of that failure, and the
    // static server's favicon 404.
    const unexpected = errors.filter(e =>
      !e.url.includes(PAK_PATH) &&
      !e.url.endsWith('/favicon.ico') &&
      !/^\[qwasm2-rs\] .*Fetch failed: 404/.test(e.text)
    );
    expect(unexpected).toEqual([]);
  });
});
