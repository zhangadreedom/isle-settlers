import { spawnSync } from 'node:child_process';
import { readFileSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const manifest = require.resolve('vite/package.json');
const { bin } = JSON.parse(readFileSync(manifest, 'utf8'));
const vite = resolve(dirname(manifest), bin.vite);
const cwd = fileURLToPath(new URL('../', import.meta.url));
const result = spawnSync(process.execPath, [vite, 'build', '--base', './', '--outDir', 'dist-embed'], {
  cwd,
  env: { ...process.env, VITE_EMBED: '1' },
  stdio: 'inherit',
});
if (result.error) throw result.error;
if (result.status === 0) {
  rmSync(resolve(cwd, 'dist-embed/sw.js'), { force: true });
  rmSync(resolve(cwd, 'dist-embed/manifest.webmanifest'), { force: true });
}
process.exitCode = result.status ?? 1;
