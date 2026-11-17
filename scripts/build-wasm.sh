#!/usr/bin/env bash
# Builds crates/isle-wasm into web/src/wasm (gitignored).
# Requires: rustup target wasm32-unknown-unknown, wasm-bindgen-cli matching
# the wasm-bindgen version in crates/isle-wasm/Cargo.toml. wasm-opt is used
# when available (pnpm installs it via the binaryen package).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build -p isle-wasm --target wasm32-unknown-unknown --profile wasm-release
wasm-bindgen target/wasm32-unknown-unknown/wasm-release/isle_wasm.wasm \
  --out-dir web/src/wasm --target web

WASM=web/src/wasm/isle_wasm_bg.wasm
WASM_OPT="$(command -v wasm-opt || true)"
if [ -z "$WASM_OPT" ] && [ -x node_modules/.bin/wasm-opt ]; then
  WASM_OPT=node_modules/.bin/wasm-opt
fi
if [ -n "$WASM_OPT" ]; then
  "$WASM_OPT" -Os --enable-bulk-memory --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-mutable-globals --enable-reference-types \
    --enable-multivalue "$WASM" -o "$WASM"
else
  echo "note: wasm-opt not found, skipping size optimization" >&2
fi
printf 'isle_wasm_bg.wasm: %s bytes (%s gzipped)\n' "$(wc -c < "$WASM")" "$(gzip -9c "$WASM" | wc -c)"
