#!/usr/bin/env bash
# Cross-compiles mw inside Docker for the given targets (the host's release target by
# default), then writes dist/<target>/ and dist/mw-<target>.tar.gz (.zip on Windows) with
# the binary and licenses (no version in the name), plus a .sha256 per archive. The release
# targets are the `dist` matrix in .github/workflows/ci.yml.
set -euo pipefail
cd "$(dirname "$0")/.."

IMAGE=ghcr.io/rust-cross/cargo-zigbuild:0.24.1

if [[ $# -eq 0 ]]; then
  host=$(rustc -vV | sed -n 's/^host: //p')
  set -- "${host/%-linux-gnu/-linux-musl}"
fi

docker run --rm -u "$(id -u):$(id -g)" -e HOME=/tmp -e CARGO_HOME=/io/target/cargo-home \
  -v "$PWD":/io -w /io "$IMAGE" \
  cargo zigbuild --release --locked "${@/#/--target=}"

rm -rf dist
for t in "$@"; do
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
