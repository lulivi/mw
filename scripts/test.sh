#!/usr/bin/env bash
# Runs the test suite, as CI does.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo test "$@"
