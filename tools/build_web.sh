#!/usr/bin/env bash
# Build the browser version into dist/ (static files; host them anywhere).
#   ./tools/build_web.sh          build
#   ./tools/build_web.sh serve    build, then serve on http://localhost:8080
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version <same as wasm-bindgen in Cargo.lock>
#        wasm-opt (binaryen) is optional but shrinks the download a lot.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --profile web --target wasm32-unknown-unknown
rm -rf dist && mkdir -p dist
wasm-bindgen --target web --no-typescript --out-dir dist \
  target/wasm32-unknown-unknown/web/bearacade2000.wasm
if command -v wasm-opt >/dev/null; then
  # Only features every current browser ships (not --all-features, which
  # switches on experimental encodings browsers reject).
  wasm-opt -Os --strip-debug --strip-producers \
    --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals --enable-reference-types --enable-multivalue \
    -o dist/opt.wasm dist/bearacade2000_bg.wasm
  mv dist/opt.wasm dist/bearacade2000_bg.wasm
fi
cp web/index.html dist/
cp -R assets dist/assets
du -sh dist dist/bearacade2000_bg.wasm

if [[ "${1:-}" == "serve" ]]; then
  echo "http://localhost:8080"
  python3 -m http.server 8080 --directory dist
fi
