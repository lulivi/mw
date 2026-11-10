#!/usr/bin/env bash
# Checks formatting, clippy lints and the shell scripts, as CI does.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
shellcheck scripts/*.sh
