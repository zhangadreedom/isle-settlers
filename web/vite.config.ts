import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

export default defineConfig({
  plugins: [solid()],
  resolve: {
    // Three.js addons import 'three'; point them at the WebGPU build so a
    // single copy of the library ships.
    alias: [{ find: /^three$/, replacement: 'three/webgpu' }],
  },
  build: {
    target: 'es2022',
    // The Three.js chunk is lazy-loaded after first paint.
    chunkSizeWarningLimit: 1000,
  },
});
