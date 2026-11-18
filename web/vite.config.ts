import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
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
  build: {
    target: 'es2022',
    // The Three.js chunk is lazy-loaded after first paint.
    chunkSizeWarningLimit: 1000,
  },
});
