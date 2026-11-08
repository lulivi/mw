#!/usr/bin/env bash
# Regenerates the sample images in docs/samples/ from a fictional listening history.
set -euo pipefail
cd "$(dirname "$0")/.."

data=$(mktemp -d)
trap 'rm -rf "$data"' EXIT

cargo build --release
cargo run --release --example sample_data -- "$data"

mw() {
  ./target/release/mw --spotify "$data/spotify" \
    --ytm "$data/watch-history.json" --overwrite "$@" >/dev/null
}

mkdir -p docs/samples
mw --source spotify --year 2025 --out docs/samples/year.png
mw --from 2024-06-01 --to 2025-05-31 --color purple --out docs/samples/range.png
mw --color blue --out docs/samples/history.png
mw --year 2025 --rank-by minutes --color green --out docs/samples/minutes.png
ls -l docs/samples
