import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gzipSync } from 'node:zlib';

const root = fileURLToPath(new URL('../', import.meta.url));
const target = 'wasm32-unknown-unknown';
const profile = 'wasm-release';
const outDir = join(root, 'web', 'src', 'wasm');

function run(command, args, capture = false) {
  const result = spawnSync(command, args, {
    cwd: root,
    encoding: 'utf8',
    stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`${command} failed (${result.signal ?? `exit ${result.status}`})`);
  }
  return result.stdout;
}

const { target_directory } = JSON.parse(run('cargo', ['metadata', '--no-deps', '--format-version', '1'], true));
run('cargo', ['build', '-p', 'isle-wasm', '--target', target, '--profile', profile]);
run('wasm-bindgen', [join(target_directory, target, profile, 'isle_wasm.wasm'), '--out-dir', outDir, '--target', 'web']);

const wasm = join(outDir, 'isle_wasm_bg.wasm');
const optimizer = join(root, 'node_modules', 'binaryen', 'bin', 'wasm-opt');
const optArgs = ['-Os', '--enable-bulk-memory', '--enable-nontrapping-float-to-int',
  '--enable-sign-ext', '--enable-mutable-globals', '--enable-reference-types',
  '--enable-multivalue', wasm, '-o', wasm];
if (existsSync(optimizer)) {
  run(process.execPath, [optimizer, ...optArgs]);
} else {
  const probe = spawnSync('wasm-opt', ['--version'], { cwd: root, stdio: 'ignore' });
  if (probe.error?.code === 'ENOENT') {
    console.warn('note: wasm-opt not found, skipping size optimization');
  } else {
    if (probe.error) throw probe.error;
    run('wasm-opt', optArgs);
  }
}
const bytes = readFileSync(wasm);
console.log(`isle_wasm_bg.wasm: ${bytes.length} bytes (${gzipSync(bytes, { level: 9 }).length} gzipped)`);
