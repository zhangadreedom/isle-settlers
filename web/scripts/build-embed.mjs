import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const manifest = require.resolve('vite/package.json');
const { bin } = JSON.parse(readFileSync(manifest, 'utf8'));
const vite = resolve(dirname(manifest), bin.vite);
const result = spawnSync(process.execPath, [vite, 'build', '--base', './', '--outDir', 'dist-embed'], {
  cwd: fileURLToPath(new URL('../', import.meta.url)),
  env: { ...process.env, VITE_EMBED: '1' },
  stdio: 'inherit',
});
if (result.error) throw result.error;
process.exitCode = result.status ?? 1;
