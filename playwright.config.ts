import { defineConfig } from '@playwright/test';

export default defineConfig({
  // Cold Vite dependency optimization on Windows can exceed the default 30 seconds.
  timeout: 60_000,
  testDir: './tests/layout',
  testMatch: '**/*.pw.ts',
  use: { baseURL: 'http://127.0.0.1:1422', viewport: { width: 800, height: 900 } },
  webServer: { command: 'bunx vite --config tests/vite.config.ts', url: 'http://127.0.0.1:1422/tests/fixtures/editor-layout.html' },
});
