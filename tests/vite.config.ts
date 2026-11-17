import { defineConfig } from 'vite';
import tailwindcss from '@tailwindcss/vite';

// Serve the real editor modules without bootstrapping Tauri or the Svelte workspace.
export default defineConfig({
  cacheDir: 'node_modules/.vite-layout',
  optimizeDeps: { entries: ['tests/fixtures/editor-layout.html'] },
  plugins: [tailwindcss()], server: { host: '127.0.0.1', port: 1422, strictPort: true }
});
