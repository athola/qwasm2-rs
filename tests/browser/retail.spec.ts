import { test, expect, Page } from '@playwright/test';
import fs from 'fs';
import http from 'http';
import { AddressInfo } from 'net';
import path from 'path';

// The browser half of the save/load dogfood (#15), on the user's own retail
// paks merged by `make pak-web-retail` (never committed or published). It
// skips without them, so CI, which has only the demo, never runs it.
const RETAIL_PAK = path.resolve(__dirname, '../../gamedata/retail/baseq2/pak0-web.pak');
const PAK_PATH = '/gamedata/baseq2/pak0-web.pak';
const DIST = path.resolve(__dirname, '../../dist');

/// dist/ with the retail web pak at the page's pak path, streamed from
/// disk: page.route would push the 196 MB pak through Playwright's protocol
/// channel, far too slowly for the page to start.
function serve(): Promise<http.Server> {
  const server = http.createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://localhost');
    const file = url.pathname === PAK_PATH
      ? RETAIL_PAK
      : path.join(DIST, url.pathname === '/qwasm2' ? 'qwasm2.html' : path.normalize(url.pathname));
    if (!file.startsWith(DIST) && file !== RETAIL_PAK) {
      res.writeHead(403).end();
      return;
    }
    fs.stat(file, (err, stat) => {
      if (err || !stat.isFile()) {
        res.writeHead(404).end();
        return;
      }
      const type = file.endsWith('.html') ? 'text/html' : 'application/octet-stream';
      res.writeHead(200, { 'content-type': type, 'content-length': stat.size });
      fs.createReadStream(file).pipe(res);
    });
  });
  return new Promise(resolve => server.listen(0, '127.0.0.1', () => resolve(server)));
}

let server: http.Server;
let origin = '';

/// The campaign's maps, from the pak directory: every maps/*.bsp but the
/// deathmatch ones (q2dm*, match1) the patches added.
function campaignMaps(): string[] {
  const pak = fs.readFileSync(RETAIL_PAK);
  const dirOffset = pak.readUInt32LE(4);
  const dirLength = pak.readUInt32LE(8);
  const maps: string[] = [];
  for (let at = dirOffset; at < dirOffset + dirLength; at += 64) {
    const name = pak.toString('latin1', at, at + 56).replace(/\0.*$/s, '');
    const map = /^maps\/(.+)\.bsp$/i.exec(name)?.[1].toLowerCase();
    if (map && !/^(q2dm\d+|match\d+)$/.test(map)) maps.push(map);
  }
  return maps.sort();
}

async function frames(page: Page, count: number) {
  await page.evaluate(n => new Promise<void>(resolve => {
    let seen = 0;
    const step = () => (++seen >= n ? resolve() : requestAnimationFrame(step));
    requestAnimationFrame(step);
  }), count);
}

/// Hold `key` across frames so the game loop sees it go down once.
async function press(page: Page, key: string) {
  await page.keyboard.down(key);
  await frames(page, 5);
  await page.keyboard.up(key);
  await frames(page, 2);
}

/// Open `map` and wait for the engine.
async function open(page: Page, map: string) {
  await page.goto(`${origin}/qwasm2?map=${map}`);
  await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 120000 });
  await expect(page.locator('#status .fail')).toHaveCount(0);
  await frames(page, 30);
}

/// The page's console lines, where the engine's prints and the server's
/// save messages go (C Com_Printf), collected per page.
const consoles = new WeakMap<Page, string[]>();

function watchConsole(page: Page) {
  const lines: string[] = [];
  consoles.set(page, lines);
  page.on('console', msg => lines.push(msg.text()));
}

/// The console lines logged since `before` lines.
function linesSince(page: Page, before: number): string {
  return (consoles.get(page) ?? []).slice(before).join('\n');
}

function lineCount(page: Page): number {
  return (consoles.get(page) ?? []).length;
}

/// F6, then F9: the save is written and loaded back into `map`.
async function quickSaveAndLoad(page: Page, map: string) {
  let before = lineCount(page);
  await press(page, 'F6');
  await expect.poll(() => linesSince(page, before)).toMatch(/Saving game\.\.\.[\s\S]*Done\./);
  before = lineCount(page);
  await press(page, 'F9');
  await expect.poll(() => linesSince(page, before), { timeout: 60000 })
    .toMatch(new RegExp(`Loading game\\.\\.\\.[\\s\\S]*Level exit to ${map}\\b`));
  await frames(page, 30);
  await expect(page.locator('#status .fail')).toHaveCount(0);
}

test.describe('Qwasm2 save and load on the retail campaign', () => {
  test.skip(!fs.existsSync(RETAIL_PAK), 'no retail web pak (make pak-web-retail)');
  test.beforeAll(async () => {
    server = await serve();
    origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  });
  test.afterAll(() => new Promise<void>(resolve => server.close(() => resolve())));
  test.beforeEach(({ page }) => watchConsole(page));

  test('a quick save outlives a page reload and loads back', async ({ page }) => {
    test.setTimeout(300000);
    await open(page, 'base1');
    const before = lineCount(page);
    await press(page, 'F6');
    await expect.poll(() => linesSince(page, before)).toMatch(/Done\./);
    // Let the page's store finish its background writes.
    await frames(page, 30);

    await page.reload();
    await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 120000 });
    await frames(page, 30);
    const reloaded = lineCount(page);
    await press(page, 'F9');
    await expect.poll(() => linesSince(page, reloaded), { timeout: 60000 })
      .toMatch(/Loading game\.\.\.[\s\S]*Level exit to base1\b/);
    await expect(page.locator('#status .fail')).toHaveCount(0);
  });

  test('F3 loads the newest save', async ({ page }) => {
    test.setTimeout(300000);
    await open(page, 'base2');
    let before = lineCount(page);
    await press(page, 'F6');
    await expect.poll(() => linesSince(page, before)).toMatch(/Done\./);
    before = lineCount(page);
    await press(page, 'F3');
    await expect.poll(() => linesSince(page, before), { timeout: 60000 })
      .toMatch(/Loading game\.\.\.[\s\S]*Level exit to base2\b/);
    await expect(page.locator('#status .fail')).toHaveCount(0);
  });

  test('every campaign map loads, saves and loads back', async ({ page }) => {
    const maps = campaignMaps();
    expect(maps.length).toBe(39);
    test.setTimeout(maps.length * 120000);
    for (const map of maps) {
      await test.step(map, async () => {
        await open(page, map);
        await quickSaveAndLoad(page, map);
      });
    }
  });
});
