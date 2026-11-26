import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';
import pkg from './package.json';

export default defineConfig({
  plugins: [solid()],
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  resolve: {
    // Three.js addons import 'three'; point them at the WebGPU build so a
    // single copy of the library ships.
    alias: [{ find: /^three$/, replacement: 'three/webgpu' }],
  },
  server: {
    // `cargo run -p isle-server` listens on 8080; the dev server forwards
    // the game protocol to it.
    proxy: { '/api': { target: 'http://localhost:8080', ws: true } },
  },
  // ES-module workers, so the WASM glue can resolve its file via import.meta.url.
  worker: { format: 'es' },
  build: {
    target: 'es2022',
    // The Three.js chunk is lazy-loaded after first paint.
    chunkSizeWarningLimit: 1000,
  },
});
