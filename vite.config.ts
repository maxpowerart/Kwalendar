import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  root: 'ui',
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { outDir: '../dist', emptyOutDir: true, target: 'es2022', cssMinify: true },
});
