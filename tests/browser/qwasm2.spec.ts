import { test, expect, Page } from '@playwright/test';
import fs from 'fs';
import path from 'path';

// dist/qwasm2.html is served by the webServer in playwright.config.ts. That
// server has no /gamedata, so start_game ends with the pak fetch 404 unless a
// test routes the pak request itself.
const HTML_PATH = 'qwasm2.html';
const PAK_PATH = '/gamedata/baseq2/pak0-web.pak';
// pak0-web.pak holds the same files as pak0.pak, so the demo pak can stand in.
const DEMO_PAK = path.resolve(__dirname, '../../gamedata/baseq2/pak0.pak');

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
    await expect(page.locator('#status')).toContainText('WebGL2 context created');
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

  test('CP-0: A refused clipboard write shows Copy failed', async ({ page }) => {
    const rejections: string[] = [];
    page.on('pageerror', err => rejections.push(err.message));
    await loadUntilSettled(page);
    await page.evaluate(() => {
      navigator.clipboard.writeText = () => Promise.reject(new DOMException('denied', 'NotAllowedError'));
    });
    await page.evaluate(() => (window as unknown as { copyLog: () => void }).copyLog());
    await expect(page.locator('#status-toolbar button').first()).toHaveText('Copy failed');
    expect(rejections).toEqual([]);
  });
});

test.describe('Qwasm2 with the demo pak', () => {
  // CI restores the pak from cache, so a missing pak there fails instead of skipping.
  test.skip(!process.env.CI && !fs.existsSync(DEMO_PAK),
    'gamedata/baseq2/pak0.pak not installed (make gamedata)');

  test('CP-5: Demo map loads and renders frames without errors', async ({ page }) => {
    test.setTimeout(90000);
    const errors: { text: string; url: string }[] = [];
    page.on('console', msg => {
      if (msg.type() === 'error') {
        errors.push({ text: msg.text(), url: msg.location().url });
      }
    });
    page.on('pageerror', err => errors.push({ text: err.message, url: '' }));
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));

    await page.goto(HTML_PATH);
    await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 60000 });
    await expect(page.locator('#status .fail')).toHaveCount(0);
    await expect(page.locator('#status')).toContainText('engine running');
    // The renderer is created once the pak is indexed (C inits the
    // filesystem before R_Init).
    await expect(page.locator('#status')).toContainText('GL3 renderer initialized');

    // Let the game loop draw a few frames; frame errors and panics go to console.error.
    await page.evaluate(() => new Promise<void>(resolve => {
      let frames = 0;
      const step = () => (++frames >= 10 ? resolve() : requestAnimationFrame(step));
      requestAnimationFrame(step);
    }));
    // The static server's favicon 404 is the only expected error.
    expect(errors.filter(e => !e.url.endsWith('/favicon.ico'))).toEqual([]);
  });

  test('CP-5: Gamma starts from ?gamma= and F8/F7 step it', async ({ page }) => {
    test.setTimeout(90000);
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));
    const frames = (n: number) => page.evaluate(count => new Promise<void>(resolve => {
      let seen = 0;
      const step = () => (++seen >= count ? resolve() : requestAnimationFrame(step));
      requestAnimationFrame(step);
    }), n);

    // `serve` redirects qwasm2.html to /qwasm2 without the query string.
    await page.goto('qwasm2?gamma=1.5');
    await expect(page.locator('#status')).toContainText('engine running', { timeout: 60000 });
    await expect(page.locator('#status')).toContainText('Gamma: 1.5');

    // Held across frames so the game loop sees the key go down once.
    await page.keyboard.down('F8');
    await frames(5);
    await page.keyboard.up('F8');
    await expect(page.locator('#status')).toContainText('Gamma: 1.6');
    await page.keyboard.down('F7');
    await frames(5);
    await page.keyboard.up('F7');
    await frames(2);
    await expect(page.locator('#status span').last()).toHaveText(/Gamma: 1\.5/);
  });

  test('CP-5: Audio context runs after a click on the canvas, without errors', async ({ page }) => {
    test.setTimeout(90000);
    const errors: { text: string; url: string }[] = [];
    page.on('console', msg => {
      // The WebAudio backend reports failed voices as warnings; only the
      // pointer-lock refusal and the autoplay notice are expected.
      const warned = msg.type() === 'warning' &&
        !/pointer lock|AudioContext was not allowed to start/i.test(msg.text());
      if (msg.type() === 'error' || warned) {
        errors.push({ text: msg.text(), url: msg.location().url });
      }
    });
    page.on('pageerror', err => errors.push({ text: err.message, url: '' }));
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));

    await page.goto(HTML_PATH);
    await expect(page.locator('#status')).toContainText('engine running', { timeout: 60000 });
    // The engine reports the context's first state; browsers may start it
    // suspended until a user gesture.
    await expect(page.locator('#status')).toContainText(/Audio: context (suspended|running)/);
    // Off-center: the "Click to play" prompt sits over the canvas middle.
    await page.locator('#canvas').click({ position: { x: 10, y: 10 } });
    await expect(page.locator('#status')).toContainText('Audio: context running');

    // Frames with sound mixing on.
    await page.evaluate(() => new Promise<void>(resolve => {
      let frames = 0;
      const step = () => (++frames >= 30 ? resolve() : requestAnimationFrame(step));
      requestAnimationFrame(step);
    }));
    expect(errors.filter(e => !e.url.endsWith('/favicon.ico'))).toEqual([]);
  });
});
