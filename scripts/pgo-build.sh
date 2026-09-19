#!/usr/bin/env bash
set -euo pipefail

ext=""
native() { printf '%s' "$1"; }
case "$(uname -s)" in
    MINGW*|MSYS*|CYGWIN*)
        ext=".exe"
        native() { cygpath -w "$1"; }
        ;;
esac

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <a tree to count> [more trees]" >&2
    echo "The repository and its language fixtures are counted anyway, but they are four" >&2
    echo "milliseconds of work, and a profile that small decides nothing: two builds from" >&2
    echo "it landed five percent apart. Name a real corpus." >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
rm -rf target-pgo pgo-data pgo.profdata

CARGO_TARGET_DIR=target-pgo RUSTFLAGS="-Cprofile-generate=$(native "$root/pgo-data")" \
        cargo build --release -p mezura

counter="target-pgo/release/mezura$ext"
for pass in 1 2 3; do
    "$counter" . > /dev/null
    "$counter" mezura-core/tests/fixtures > /dev/null
    "$counter" . --counting region --hide keywords > /dev/null
    for target in "$@"; do
        "$counter" "$target" > /dev/null
    done
done

sysroot="$(rustc --print sysroot)"
case "$ext" in .exe) sysroot="$(cygpath -u "$sysroot")" ;; esac
profdata="$(find "$sysroot" -name "llvm-profdata$ext" -type f | head -1)"
if [ -z "$profdata" ]; then
    echo "llvm-profdata is not installed: rustup component add llvm-tools" >&2
    exit 1
fi
"$profdata" merge -o "$root/pgo.profdata" pgo-data

RUSTFLAGS="-Cprofile-use=$(native "$root/pgo.profdata")" cargo build --release

rm -rf target-pgo pgo-data
