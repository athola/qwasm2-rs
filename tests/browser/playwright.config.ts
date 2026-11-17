import { defineConfig } from '@playwright/test';
import path from 'path';

const distDir = path.resolve(__dirname, '../../dist');
// Not 3000, which other dev servers default to and reuse would pick up.
const port = Number(process.env.QWASM2_TEST_PORT ?? 3917);

export default defineConfig({
  testDir: '.',
  timeout: 30000,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: `http://localhost:${port}`,
    trace: 'retain-on-failure',
    launchOptions: {
      args: ['--use-gl=angle', '--use-angle=swiftshader'],
    },
  },
  webServer: {
    command: `npx serve "${distDir}" -l ${port} --no-clipboard`,
    port,
    reuseExistingServer: false,
    timeout: 10000,
  },
});
