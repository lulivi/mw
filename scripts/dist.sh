#!/usr/bin/env bash
# Cross-compiles mw for every release target inside Docker and copies the binaries to dist/.
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=ghcr.io/rust-cross/cargo-zigbuild:0.24.1
TARGETS=(aarch64-apple-darwin x86_64-pc-windows-gnu x86_64-unknown-linux-musl)

docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -e CARGO_HOME=/io/target/cargo-home \
  -v "$PWD":/io -w /io "$IMAGE" \
  cargo zigbuild --release --locked "${TARGETS[@]/#/--target=}"

mkdir -p dist
for t in "${TARGETS[@]}"; do
  ext=; [[ $t == *windows* ]] && ext=.exe
  cp "target/$t/release/mw$ext" "dist/mw-$t$ext"
done
ls -l dist
