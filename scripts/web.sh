#!/usr/bin/env bash
# Builds the web page into target/web: the files in web/www plus the library
# compiled to WebAssembly. Serve it with e.g. `python3 -m http.server -d target/web`.
#
# Needs the wasm32-unknown-unknown target and wasm-bindgen-cli of the version
# web/Cargo.toml pins:
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version <version> --locked
set -euo pipefail
cd "$(dirname "$0")/.."

out=target/web
cargo build -p mw-web --profile web --target wasm32-unknown-unknown
rm -rf "$out"
mkdir -p "$out"
cp -r web/www/. "$out"
# The placeholder shown before the first image.
cp docs/samples/year.png "$out/sample.png"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" \
  target/wasm32-unknown-unknown/web/mw_web.wasm
ls -l "$out" "$out/pkg"
