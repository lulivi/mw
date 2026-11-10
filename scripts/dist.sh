#!/usr/bin/env bash
# Cross-compiles mw for every release target inside Docker, then writes dist/<target>/ and
# dist/mw-<target>.tar.gz (.zip on Windows) with the binary and licenses (no version in the
# name), plus a .sha256 per archive.
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=ghcr.io/rust-cross/cargo-zigbuild:0.24.1
TARGETS=(
  aarch64-apple-darwin x86_64-apple-darwin
  x86_64-pc-windows-gnu
  x86_64-unknown-linux-musl aarch64-unknown-linux-musl
)

docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -e CARGO_HOME=/io/target/cargo-home \
  -v "$PWD":/io -w /io "$IMAGE" \
  cargo zigbuild --release --locked "${TARGETS[@]/#/--target=}"

rm -rf dist
for t in "${TARGETS[@]}"; do
  ext=; [[ $t == *windows* ]] && ext=.exe
  mkdir -p "dist/$t"
  cp "target/$t/release/mw$ext" LICENSE assets/OFL-*.txt "dist/$t/"
  if [[ $t == *windows* ]]; then
    archive=mw-$t.zip
    (cd "dist/$t" && zip -q "../$archive" -- *)
  else
    archive=mw-$t.tar.gz
    (cd "dist/$t" && tar -czf "../$archive" -- *)
  fi
  (cd dist && sha256sum "$archive" > "$archive.sha256")
done
ls -l dist
