#!/usr/bin/env bash
set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export KIND_PREFIX="${KIND_PREFIX:-results-linux-native}"
export OUTROOT="${OUTROOT:-$SCRIPT_DIR}"

exec bash "$SCRIPT_DIR/run-benchmarks-linux.sh" "$@"
