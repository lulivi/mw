#!/usr/bin/env bash
# Checks formatting and clippy lints, as CI does.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
