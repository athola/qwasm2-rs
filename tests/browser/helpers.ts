import { expect, Page } from '@playwright/test';

export async function frames(page: Page, count: number) {
  await page.evaluate(n => new Promise<void>(resolve => {
    let seen = 0;
    const step = () => (++seen >= n ? resolve() : requestAnimationFrame(step));
    requestAnimationFrame(step);
  }), count);
}

/// Hold `key` across frames so the game loop sees it go down once.
export async function press(page: Page, key: string) {
  await page.keyboard.down(key);
  await frames(page, 5);
  await page.keyboard.up(key);
  await frames(page, 2);
}

/// Run console text in the page.
export async function send(page: Page, text: string) {
  await page.evaluate(t =>
    document.dispatchEvent(new CustomEvent('qwasm2-command', { detail: t })), text);
}

/// Wait until the page's save store has finished every write.
export async function stored(page: Page) {
  await expect.poll(() => page.evaluate(() =>
    (window as Window & { qwasm2Saves: { pending: number } }).qwasm2Saves.pending)).toBe(0);
}

/// What a load prints before its level exit line.
export const LOADING = 'Loading game\\.\\.\\.[\\s\\S]*';
