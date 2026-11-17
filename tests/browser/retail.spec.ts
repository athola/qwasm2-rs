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

let directory: Map<string, [number, number]> | undefined;

/// Every pak file's (offset, size) by lower-cased name, read once.
function pakDirectory(): Map<string, [number, number]> {
  if (directory) return directory;
  const pak = fs.readFileSync(RETAIL_PAK);
  const dir = new Map<string, [number, number]>();
  const dirOffset = pak.readUInt32LE(4);
  for (let at = dirOffset; at < dirOffset + pak.readUInt32LE(8); at += 64) {
    const name = pak.toString('latin1', at, at + 56).replace(/\0.*$/s, '').toLowerCase();
    dir.set(name, [pak.readUInt32LE(at + 56), pak.readUInt32LE(at + 60)]);
  }
  return (directory = dir);
}

/// The campaign's maps: every maps/*.bsp but the deathmatch ones (q2dm*,
/// match1) the patches added.
function campaignMaps(): string[] {
  return [...pakDirectory().keys()]
    .map(name => /^maps\/(.+)\.bsp$/.exec(name)?.[1])
    .filter((map): map is string => !!map && !/^(q2dm\d+|match\d+)$/.test(map))
    .sort();
}

/// The `map` keys of a map's target_changelevels, from its BSP's entity lump.
function exits(map: string): string[] {
  const [offset] = pakDirectory().get(`maps/${map}.bsp`)!;
  const fd = fs.openSync(RETAIL_PAK, 'r');
  const header = Buffer.alloc(16);
  fs.readSync(fd, header, 0, 16, offset);
  const [lumpOffset, lumpLength] = [header.readUInt32LE(8), header.readUInt32LE(12)];
  const text = Buffer.alloc(lumpLength);
  fs.readSync(fd, text, 0, lumpLength, offset + lumpOffset);
  fs.closeSync(fd);
  const levels = new Set<string>();
  for (const entity of text.toString('latin1').split('}')) {
    if (/"classname"\s+"target_changelevel"/.test(entity)) {
      const level = /"map"\s+"([^"]+)"/.exec(entity)?.[1];
      if (level) levels.add(level);
    }
  }
  return [...levels].sort();
}

/// The game map a level string ends in (following a cinematic's next
/// server), or null for a picture.
function destination(level: string): string | null {
  const plus = level.indexOf('+');
  const head = plus < 0 ? level : level.slice(0, plus);
  const map = head.split('$')[0].replace(/^\*/, '');
  if (map.endsWith('.cin')) return plus < 0 ? null : destination(level.slice(plus + 1));
  return map.endsWith('.pcx') || map.endsWith('.dm2') ? null : map;
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

/// Wait for the engine the page just loaded.
async function ready(page: Page) {
  await expect(page.locator('#status')).toContainText(/ERROR:|engine running/, { timeout: 120000 });
  await expect(page.locator('#status .fail')).toHaveCount(0);
  await frames(page, 30);
}

async function open(page: Page, map: string) {
  await page.goto(`${origin}/qwasm2?map=${map}`);
  await ready(page);
}

/// The test page's console lines, where the engine's prints and the
/// server's save messages go (C Com_Printf).
let lines: string[] = [];

function linesSince(before: number): string {
  return lines.slice(before).join('\n');
}

/// Run console text in the page.
async function send(page: Page, text: string) {
  await page.evaluate(t =>
    document.dispatchEvent(new CustomEvent('qwasm2-command', { detail: t })), text);
}

/// Wait for the load that `before` lines started to reach `map`.
async function arrive(page: Page, map: string, before: number, loading = '') {
  await expect.poll(() => linesSince(before), { timeout: 60000 })
    .toMatch(new RegExp(`${loading}Level exit to \\*?${map}\\b`));
  await frames(page, 20);
  await expect(page.locator('#status .fail')).toHaveCount(0);
}

const LOADING = 'Loading game\\.\\.\\.[\\s\\S]*';

/// Wait until the page's save store has finished every write.
async function stored(page: Page) {
  await expect.poll(() => page.evaluate(() =>
    (window as Window & { qwasm2Saves: { pending: number } }).qwasm2Saves.pending)).toBe(0);
}

/// F6, then F9: the save is written and loaded back into `map`.
async function quickSaveAndLoad(page: Page, map: string) {
  let before = lines.length;
  await press(page, 'F6');
  await expect.poll(() => linesSince(before)).toMatch(/Saving game\.\.\.[\s\S]*Done\./);
  before = lines.length;
  await press(page, 'F9');
  await arrive(page, map, before, LOADING);
}

test.describe('Qwasm2 save and load on the retail campaign', () => {
  test.skip(!fs.existsSync(RETAIL_PAK), 'no retail web pak (make pak-web-retail)');
  test.beforeAll(async () => {
    server = await serve();
    origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  });
  test.afterAll(() => new Promise<void>(resolve => server.close(() => resolve())));
  test.beforeEach(({ page }) => {
    const pageLines: string[] = [];
    lines = pageLines;
    page.on('console', msg => pageLines.push(msg.text()));
  });

  test('a quick save outlives a page reload and loads back', async ({ page }) => {
    test.setTimeout(300000);
    await open(page, 'base1');
    const before = lines.length;
    await press(page, 'F6');
    await expect.poll(() => linesSince(before)).toMatch(/Done\./);
    await stored(page);

    // A new visit on another map: only the kept save can bring base1 back.
    await open(page, 'base2');
    const reloaded = lines.length;
    await press(page, 'F9');
    await arrive(page, 'base1', reloaded, LOADING);
  });

  test('the autosave outlives a page reload', async ({ page }) => {
    // Entering base2 writes save0. A reload starts a new game, which keeps
    // the last visit's save0 (C's map would write over it); F3 loads it.
    test.setTimeout(300000);
    await open(page, 'base1');
    const before = lines.length;
    await send(page, 'gamemap base2$base1');
    await arrive(page, 'base2', before);
    await stored(page);

    await page.reload();
    await ready(page);
    const reloaded = lines.length;
    await press(page, 'F3');
    await arrive(page, 'base2', reloaded, LOADING);
  });

  test('the campaign plays through every exit with saves', async ({ page }) => {
    // The browser twin of q2-server's campaign_walk: every target_changelevel
    // exit taken with gamemap, as C ExitLevel does, a quick save and load
    // on each level reached, a death that loads the newest save, and a page
    // reload in the middle that keeps the unit's saves.
    const graph = new Map<string, string[]>();
    const queue = ['base1'];
    while (queue.length) {
      const map = queue.shift()!;
      if (graph.has(map)) continue;
      graph.set(map, exits(map));
      for (const level of graph.get(map)!) {
        const next = destination(level);
        if (next) queue.push(next);
      }
    }
    const untaken = new Set([...graph].flatMap(([map, levels]) => levels.map(l => `${map} ${l}`)));
    test.setTimeout(untaken.size * 90000);
    const firstStep = (from: string): string | null => {
      // Breadth first toward the nearest level with an exit not taken.
      const first = new Map<string, string>();
      const seen = new Set([from]);
      const q = [from];
      while (q.length) {
        const map = q.shift()!;
        if (map !== from && [...untaken].some(e => e.startsWith(`${map} `))) return first.get(map)!;
        for (const level of graph.get(map) ?? []) {
          const next = destination(level);
          if (next && !seen.has(next)) {
            seen.add(next);
            first.set(next, first.get(map) ?? level);
            q.push(next);
          }
        }
      }
      return null;
    };

    await open(page, 'base1');
    let here = 'base1';
    let exitsTaken = 0;
    let died = false;
    let reloaded = false;
    for (;;) {
      const edge = [...untaken].find(e => e.startsWith(`${here} `));
      const level = edge ? edge.slice(here.length + 1) : firstStep(here);
      if (!level) break;
      untaken.delete(`${here} ${level}`);
      const before = lines.length;
      await send(page, `gamemap ${level}`);
      const next = destination(level);
      if (!next) {
        // The campaign's end picture: nothing to save there.
        await expect.poll(() => linesSince(before), { timeout: 60000 }).toMatch(/Level exit to/);
        break;
      }
      await arrive(page, next, before);
      here = next;
      exitsTaken++;
      await quickSaveAndLoad(page, here);

      if (!died && exitsTaken === 3) {
        // A press after the death delay pushes menu_loadgame: the newest
        // save, the quick save just made here. C Cmd_Kill_f ignores kill
        // for five seconds of game time after a spawn, which frames cannot
        // measure, so kill and press until the load starts.
        const deadAt = lines.length;
        await expect.poll(async () => {
          await send(page, 'kill');
          await frames(page, 10);
          await press(page, 'Control');
          return linesSince(deadAt);
        }, { timeout: 60000 }).toMatch(/Loading game/);
        await arrive(page, here, deadAt, LOADING);
        died = true;
      }
      if (!reloaded && exitsTaken === 6) {
        // A reload starts a new game; the quick save of this level stays.
        await stored(page);
        await page.reload();
        await ready(page);
        const fresh = lines.length;
        await press(page, 'F9');
        await arrive(page, here, fresh, LOADING);
        reloaded = true;
      }
    }
    expect(died && reloaded).toBe(true);
    expect([...untaken]).toEqual([]);
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
