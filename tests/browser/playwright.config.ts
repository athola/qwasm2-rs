import { defineConfig } from '@playwright/test';
import path from 'path';

const distDir = path.resolve(__dirname, '../../dist');

export default defineConfig({
  testDir: '.',
  timeout: 30000,
  use: {
    baseURL: 'http://localhost:3000',
  },
  webServer: {
    command: `npx serve "${distDir}" -l 3000 --no-clipboard`,
    port: 3000,
    reuseExistingServer: !process.env.CI,
    timeout: 10000,
  },
});
