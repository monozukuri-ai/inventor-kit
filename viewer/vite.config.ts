import { defineConfig } from 'vite';

export default defineConfig({
  base: './',
  build: {
    outDir: '../python/inventor_kit/viewer/static', emptyOutDir: true,
    chunkSizeWarningLimit: 3000,
  },
});
