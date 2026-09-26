import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  build: {
    outDir: 'dist',
    target: 'es2022',
    chunkSizeWarningLimit: 1200,
  },
  server: {
    proxy: {
      '/replays': 'http://127.0.0.1:8321',
      '/api': 'http://127.0.0.1:8321',
    },
  },
});
