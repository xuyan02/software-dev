import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  // The SPA is served under this prefix by the control plane; the same prefix
  // is used by the Vite dev server so routes and assets behave identically.
  base: '/workflows/editor/',
  plugins: [react()],
  server: {
    proxy: {
      '/api': 'http://127.0.0.1:8080',
      '/login': 'http://127.0.0.1:8080',
    },
  },
  build: {
    // React Flow plus the editor UI is one page; splitting buys little here.
    chunkSizeWarningLimit: 700,
  },
  test: {
    environment: 'node',
  },
});
