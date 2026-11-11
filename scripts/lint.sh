#!/usr/bin/env bash
# Checks formatting, clippy lints and the shell scripts, as CI does.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
shellcheck scripts/*.sh
