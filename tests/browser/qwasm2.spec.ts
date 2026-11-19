import { test, expect, Page } from '@playwright/test';
import fs from 'fs';
import path from 'path';
import { frames, LOADING, press, send, stored } from './helpers';

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

  test('CP-0: A save store that will not open is reported on the page', async ({ page }) => {
    await page.addInitScript(() => {
      indexedDB.open = () => { throw new DOMException('storage denied', 'SecurityError'); };
    });
    await loadUntilSettled(page);
    await expect(page.locator('#status .fail').first())
      .toHaveText(/saves will last only this visit.*storage denied/i);
  });

  test('CP-0: A second tab keeps its saves for the visit and says so', async ({ page, context }) => {
    await loadUntilSettled(page);
    const second = await context.newPage();
    await loadUntilSettled(second);
    await expect(second.locator('#status .fail').first())
      .toHaveText(/saves will last only this visit.*another tab/i);
    await expect(page.locator('#status')).not.toContainText('another tab');
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

    // A frame that only cleared, or never drew, leaves the middle of the view
    // one color; the demo map's start view has thousands. Hide the DOM
    // overlays so only the canvas is sampled, and stay clear of the 2D status
    // bar at the bottom.
    await page.addStyleTag({ content: '#game-container > :not(canvas) { visibility: hidden !important; }' });
    const shot = await page.locator('#canvas').screenshot();
    const colors = await page.evaluate(async png => {
      const img = new Image();
      img.src = `data:image/png;base64,${png}`;
      await img.decode();
      const scratch = document.createElement('canvas');
      scratch.width = img.width;
      scratch.height = img.height;
      const ctx = scratch.getContext('2d')!;
      ctx.drawImage(img, 0, 0);
      const x = Math.floor(img.width / 4), y = Math.floor(img.height / 5);
      const px = ctx.getImageData(x, y, img.width / 2, img.height * 2 / 5).data;
      const seen = new Set<number>();
      for (let i = 0; i < px.length; i += 4) seen.add((px[i] << 16) | (px[i + 1] << 8) | px[i + 2]);
      return seen.size;
    }, shot.toString('base64'));
    expect(colors).toBeGreaterThan(64);
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

  /** The page with the promise the fullscreen test waits on, and its
   *  count of fullscreen requests. */
  type LockWindow = Window & { lockSettled?: Promise<Event>; fullscreenRequests?: number };

  test('CP-5: ?vid_fullscreen=1 takes the game fullscreen on the click to play', async ({ page }) => {
    test.setTimeout(90000);
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));
    const fullscreenId = () => page.evaluate(() => document.fullscreenElement?.id ?? null);
    const fullscreenRequests = () => page.evaluate(() => (window as LockWindow).fullscreenRequests);
    const clickToPlay = async (query: string) => {
      // `serve` redirects qwasm2.html to /qwasm2 without the query string.
      await page.goto('qwasm2' + query);
      await expect(page.locator('#status')).toContainText('engine running', { timeout: 60000 });
      // The click asks for pointer lock; it settles as pointerlockchange
      // (granted, as on CI's Linux Chromium) or pointerlockerror (refused,
      // as on macOS headless), which shows the click handler has run. The
      // handler asks for fullscreen first, so the count of requests is
      // final then, while fullscreenElement settles on its own later task.
      await page.evaluate(() => {
        const w = window as LockWindow;
        w.fullscreenRequests = 0;
        const request = Element.prototype.requestFullscreen;
        Element.prototype.requestFullscreen = function (this: Element, options?: FullscreenOptions) {
          w.fullscreenRequests! += 1;
          return request.call(this, options);
        };
        w.lockSettled = new Promise<Event>(resolve => {
          document.addEventListener('pointerlockchange', resolve, { once: true });
          document.addEventListener('pointerlockerror', resolve, { once: true });
        });
      });
      await page.locator('#canvas').click();
      await page.evaluate(() => (window as LockWindow).lockSettled);
    };
    // Off by default (C vid_fullscreen 0).
    await clickToPlay('');
    expect(await fullscreenRequests()).toBe(0);
    expect(await fullscreenId()).toBeNull();
    // On, the container with the HUD and console goes fullscreen.
    await clickToPlay('?vid_fullscreen=1');
    expect(await fullscreenRequests()).toBe(1);
    await expect.poll(fullscreenId).toBe('game-container');
  });

  test('CP-5: A refused fullscreen request is logged and skips the keyboard lock', async ({ page }) => {
    test.setTimeout(90000);
    const pageErrors: string[] = [];
    const warnings: string[] = [];
    page.on('pageerror', err => pageErrors.push(err.message));
    page.on('console', msg => {
      if (msg.type() === 'warning') {
        warnings.push(msg.text());
      }
    });
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));
    // `serve` redirects qwasm2.html to /qwasm2 without the query string.
    await page.goto('qwasm2?vid_fullscreen=1');
    await expect(page.locator('#status')).toContainText('engine running', { timeout: 60000 });
    // Refuse as a browser does without user activation or allowfullscreen,
    // and count keyboard lock requests.
    await page.evaluate(() => {
      const w = window as Window & { keyboardLocks?: number };
      w.keyboardLocks = 0;
      Element.prototype.requestFullscreen = () =>
        Promise.reject(new TypeError('Permissions check failed'));
      const keyboard = (navigator as Navigator & { keyboard?: { lock: () => Promise<void> } }).keyboard;
      if (keyboard) {
        keyboard.lock = () => {
          w.keyboardLocks! += 1;
          return Promise.resolve();
        };
      }
    });
    await page.locator('#canvas').click();
    await expect.poll(() => warnings.some(w => w.includes('[qwasm2] fullscreen refused'))).toBe(true);
    expect(pageErrors).toEqual([]);
    expect(await page.evaluate(() => (window as Window & { keyboardLocks?: number }).keyboardLocks)).toBe(0);
  });

  test('CP-5: Audio context runs after a click on the canvas, without errors', async ({ page }) => {
    test.setTimeout(90000);
    const errors: { text: string; url: string }[] = [];
    page.on('console', msg => {
      // The WebAudio backend reports failed voices as warnings; only the
      // pointer-lock refusal and the autoplay notice are expected.
      // Chrome's own "GL Driver Message" notes (GPU stalls under load) are
      // diagnostics from the browser, not from the page.
      const warned = msg.type() === 'warning' &&
        !/pointer lock|AudioContext was not allowed to start|GL Driver Message/i.test(msg.text());
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
    const audioAt = (text: string | null, state: string) =>
      (text ?? '').lastIndexOf(`Audio: context ${state}`);
    const first = await page.locator('#status').textContent();
    // Off-center: the "Click to play" prompt sits over the canvas middle.
    await page.locator('#canvas').click({ position: { x: 10, y: 10 } });
    if (audioAt(first, 'suspended') >= 0) {
      // The click is what started it: running is reported after suspended.
      await expect.poll(async () => {
        const text = await page.locator('#status').textContent();
        return audioAt(text, 'running') > audioAt(text, 'suspended');
      }).toBe(true);
    } else {
      test.info().annotations.push({
        type: 'note',
        description: 'this browser started audio unsuspended; the click was not needed',
      });
      await expect(page.locator('#status')).toContainText('Audio: context running');
    }

    // Frames with sound mixing on.
    await page.evaluate(() => new Promise<void>(resolve => {
      let frames = 0;
      const step = () => (++frames >= 30 ? resolve() : requestAnimationFrame(step));
      requestAnimationFrame(step);
    }));
    expect(errors.filter(e => !e.url.endsWith('/favicon.ico'))).toEqual([]);
  });
});

test.describe('Qwasm2 saves with the demo pak', () => {
  test.skip(!process.env.CI && !fs.existsSync(DEMO_PAK),
    'gamedata/baseq2/pak0.pak not installed (make gamedata)');

  /// The page's console lines, where the server's save prints go.
  let lines: string[] = [];
  const since = (before: number) => lines.slice(before).join('\n');

  test.beforeEach(async ({ page }) => {
    const pageLines: string[] = [];
    lines = pageLines;
    page.on('console', msg => pageLines.push(msg.text()));
    await page.route(`**${PAK_PATH}`, route => route.fulfill({ path: DEMO_PAK }));
  });

  async function ready(page: Page) {
    await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 60000 });
    await expect(page.locator('#status .fail')).toHaveCount(0);
    await frames(page, 10);
  }

  async function save(page: Page, run: () => Promise<void>) {
    const before = lines.length;
    await run();
    await expect.poll(() => since(before)).toMatch(/Saving game\.\.\.[\s\S]*Done\./);
  }

  /// F9 after a reload: only the kept quick save can be loaded.
  async function quickLoad(page: Page) {
    const before = lines.length;
    await press(page, 'F9');
    await expect.poll(() => since(before), { timeout: 60000 })
      .toMatch(new RegExp(`${LOADING}Level exit to \\*?demo1\\b`));
  }

  test('CP-6: A quick save outlives a page reload and loads back', async ({ page }) => {
    test.setTimeout(120000);
    await page.goto(HTML_PATH);
    await ready(page);
    await save(page, () => press(page, 'F6'));
    await stored(page);

    await page.reload();
    await ready(page);
    await quickLoad(page);
    await expect(page.locator('#status .fail')).toHaveCount(0);
  });

  test('CP-6: A malformed stored save is skipped and the game starts', async ({ page }) => {
    test.setTimeout(120000);
    await page.goto(HTML_PATH);
    await ready(page);
    await page.evaluate(() => new Promise<void>((resolve, reject) => {
      const req = indexedDB.open('qwasm2-saves', 1);
      req.onerror = () => reject(req.error);
      req.onsuccess = () => {
        const tx = req.result.transaction('files', 'readwrite');
        tx.objectStore('files').put(5, 'quick/server.ssv');
        tx.oncomplete = () => { req.result.close(); resolve(); };
        tx.onabort = () => reject(tx.error);
      };
    }));

    const before = lines.length;
    await page.reload();
    await ready(page);
    await expect(page.locator('#status')).toContainText('engine running');
    expect(since(before)).toContain('quick/server.ssv');
  });

  test('CP-6: A save the store refuses is reported once and kept for the next write', async ({ page }) => {
    test.setTimeout(120000);
    await page.goto(HTML_PATH);
    await ready(page);
    // The next two puts fail as a full disk does, each in its own batch.
    await page.evaluate(() => {
      const put = IDBObjectStore.prototype.put;
      let refusals = 2;
      IDBObjectStore.prototype.put = function (this: IDBObjectStore, ...args: [unknown, IDBValidKey?]) {
        if (refusals > 0) {
          refusals--;
          throw new DOMException('disk full', 'QuotaExceededError');
        }
        return put.apply(this, args);
      };
    });
    await save(page, () => press(page, 'F6'));
    await stored(page);
    await expect(page.locator('#status .fail')).toHaveCount(1);
    await expect(page.locator('#status .fail')).toHaveText(/saves not kept.*disk full/i);
    // Refused again: still one report.
    await save(page, () => send(page, 'save other'));
    await stored(page);
    await expect(page.locator('#status .fail')).toHaveCount(1);

    // A later slot's save carries the refused quick save with it.
    await save(page, () => send(page, 'save third'));
    await stored(page);
    await expect(page.locator('#status .fail')).toHaveCount(1);

    await page.reload();
    await ready(page);
    await quickLoad(page);
  });

  test('CP-6: Saves a throwing commit missed go with the next one', async ({ page }) => {
    test.setTimeout(120000);
    await page.goto(HTML_PATH);
    await ready(page);
    await page.evaluate(() => {
      const saves = (window as Window & { qwasm2Saves: { commit: (ops: unknown) => void } }).qwasm2Saves;
      const commit = saves.commit;
      let thrown = false;
      saves.commit = function (ops: unknown) {
        if (!thrown) {
          thrown = true;
          throw new Error('store busy');
        }
        return commit.call(this, ops);
      };
    });
    await save(page, () => press(page, 'F6'));
    // The engine sends the changes again with the next frame's flush.
    await frames(page, 2);
    await stored(page);

    await page.reload();
    await ready(page);
    await quickLoad(page);
  });

  test('CP-6: Saves the store could not read are not written over', async ({ page }) => {
    test.setTimeout(120000);
    await page.addInitScript(() => {
      IDBObjectStore.prototype.openCursor = () => {
        throw new DOMException('read failed', 'UnknownError');
      };
    });
    await page.goto(HTML_PATH);
    await expect(page.locator('#status')).toContainText('engine running', { timeout: 60000 });
    await expect(page.locator('#status .fail').first()).toHaveText(/saved games not read.*read failed/i);
    await frames(page, 10);
    await save(page, () => press(page, 'F6'));
    await stored(page);
    const kept = await page.evaluate(() => new Promise<number>((resolve, reject) => {
      const req = indexedDB.open('qwasm2-saves', 1);
      req.onerror = () => reject(req.error);
      req.onsuccess = () => {
        const count = req.result.transaction('files').objectStore('files').count();
        count.onsuccess = () => resolve(count.result);
        count.onerror = () => reject(count.error);
      };
    }));
    expect(kept).toBe(0);
  });
});
