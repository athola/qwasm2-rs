import { defineConfig } from '@playwright/test';
import path from 'path';

const distDir = path.resolve(__dirname, '../../dist');

export default defineConfig({
  testDir: '.',
  timeout: 30000,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  use: {
    baseURL: 'http://localhost:3000',
    trace: 'retain-on-failure',
    launchOptions: {
      args: ['--use-gl=angle', '--use-angle=swiftshader'],
    },
  },
  webServer: {
    command: `npx serve "${distDir}" -l 3000 --no-clipboard`,
    port: 3000,
    reuseExistingServer: !process.env.CI,
    timeout: 10000,
  },
});
